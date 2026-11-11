const day = 86_400_000
const inputKinds = new Set(['function_call_output', 'custom_tool_call_output', 'mcp_call_output'])
const outputKinds = new Set(['reasoning', 'function_call', 'custom_tool_call'])
const turnStates = {
  task_started: 'running',
  task_complete: 'completed',
  turn_aborted: 'interrupted',
}

function json(text) {
  try { return JSON.parse(text) } catch { return null }
}

export function threadId(header) {
  const event = json(header)
  const meta = event?.payload
  if (event?.type !== 'session_meta' || !meta || meta.parent_thread_id != null
    || meta.source?.subagent != null || meta.source?.subAgent != null) return null
  return typeof meta.id === 'string' ? meta.id : null
}

function updateTurn(turn, event, time) {
  const payload = event.payload
  const state = turnStates[payload.type] ??
    (payload.type === 'error' && turn?.state === 'running' && payload.will_retry !== true ? 'error' : null)
  if (!state) return turn
  const turnId = typeof payload.turn_id === 'string' ? payload.turn_id : null
  if (state !== 'running' && turn?.turnId != null && turnId != null && turnId !== turn.turnId) return turn
  if (state !== 'running') return { turnId: turnId ?? turn?.turnId ?? null, state, started: null }
  return {
    turnId,
    state,
    started: Number.isSafeInteger(payload.started_at) && payload.started_at >= 0
      ? payload.started_at * 1000
      : Number.isFinite(time) ? time : null,
  }
}

export function parseRollout(text) {
  let turn = null
  let start = null
  let generated = null
  let nextInput = null
  let ambiguous = false
  let previousTotal = null
  const samples = []
  // Scan one line at a time so large JSONL tails do not create a second copy.
  for (let offset = 0; offset < text.length;) {
    const newline = text.indexOf('\n', offset)
    const end = newline < 0 ? text.length : newline
    const event = json(text.slice(offset, end))
    offset = end + 1
    const payload = event?.payload
    if (!payload) continue
    const kind = payload.type
    const time = Date.parse(event.timestamp)
    if (event.type === 'event_msg') {
      turn = updateTurn(turn, event, time)
      if (kind === 'token_count') {
        const total = payload.info?.total_token_usage?.output_tokens
        if (!Number.isSafeInteger(total) || total < 0 || previousTotal === total) continue
        previousTotal = total
        const tokens = payload.info?.last_token_usage?.output_tokens
        const durationMs = generated - start
        if (!ambiguous && start != null && generated != null && Number.isSafeInteger(tokens)
          && tokens >= 20 && durationMs >= 100 && durationMs <= 300_000) {
          samples.push({ tokens, durationMs, endedMs: generated })
        }
        start = nextInput
        generated = nextInput = null
        ambiguous = false
        continue
      }
      if (kind === 'task_complete' || kind === 'turn_aborted' || kind === 'error' || kind === 'task_started') {
        start = kind === 'task_started' && Number.isFinite(time) ? time : null
        generated = nextInput = null
        ambiguous = false
        continue
      }
    }
    if (!Number.isFinite(time)) continue
    const response = event.type === 'response_item'
    const input = (event.type === 'event_msg' && kind === 'user_message')
      || (response && (inputKinds.has(kind) || (kind === 'message' && payload.role === 'user')))
    const output = response && (outputKinds.has(kind) || (kind === 'message' && payload.role === 'assistant'))
    if (input) {
      if (generated == null) start = time
      else nextInput = time
    } else if (output) {
      ambiguous ||= nextInput != null
      generated = time
    }
  }
  return { samples: newestSamples(samples), turn }
}

function newestSamples(samples) {
  const order = (a, b) => b.endedMs - a.endedMs || b.durationMs - a.durationMs || b.tokens - a.tokens
  return samples.sort(order)
    .filter((sample, i) => i === 0 || order(sample, samples[i - 1]) !== 0)
    .slice(0, 10)
}

export function summarizeRollouts({ samples, turns, nowMs }) {
  const unique = newestSamples(samples)
  const totalTokens = unique.reduce((sum, sample) => sum + sample.tokens, 0)
  const totalMs = unique.reduce((sum, sample) => sum + sample.durationMs, 0)
  const speed = unique.length === 0 ? null : {
    tokensPerSecond: Math.round(totalTokens / (totalMs / 1000)),
    samples: unique.length,
    latestAt: Math.trunc(unique[0].endedMs / 1000),
  }
  const candidates = turns.map(({ id, state, modified, started }) => {
    const running = state === 'running' && nowMs - modified <= 180_000
    return { id, running, time: running ? started ?? modified : modified,
      state: state === 'running' && !running ? 'unknown' : state }
  })
  candidates.sort((a, b) => Number(b.running) - Number(a.running) || b.time - a.time ||
    (a.id < b.id ? 1 : a.id > b.id ? -1 : 0))
  const task = candidates[0]
  return { speed, task: task ? { id: task.id, state: task.state } : null }
}

export async function localReadings(context, cache = {}) {
  const nowMs = context.sampledAtUnixMs
  const cutoff = nowMs - 7 * day
  let recent
  try {
    recent = await context.files.find('logs', {
      path: 'sessions', prefix: 'rollout-', suffix: '.jsonl', depth: 4,
      modifiedSince: cutoff, limit: 16,
    })
  } catch {
    return { speed: null, task: null, logs: {} }
  }
  const logs = Object.create(null)
  const samples = []
  const turns = []
  for (const { path, modified, size } of recent) {
    let entry = cache[path]
    if (entry?.modified !== modified || entry?.size !== size) {
      try {
        const { head, tail } = await context.files.readWindow('logs', path, {
          headBytes: 256 * 1024, tailBytes: 2 * 1024 * 1024,
        })
        entry = { ...parseRollout(tail), id: threadId(head), modified, size }
      } catch { continue }
    }
    logs[path] = entry
    samples.push(...entry.samples.filter(sample => sample.endedMs >= cutoff))
    if (entry.id && entry.turn) turns.push({ ...entry.turn, id: entry.id, modified })
  }
  const { speed, task } = summarizeRollouts({ samples, turns, nowMs })
  if (task) {
    task.title = null
    try {
      const { tail } = await context.files.readWindow('logs', 'session_index.jsonl', {
        tailBytes: 2 * 1024 * 1024,
      })
      for (const line of tail.split('\n')) {
        const entry = json(line)
        if (entry?.id === task.id && typeof entry.thread_name === 'string') {
          task.title = entry.thread_name.trim() ? entry.thread_name : null
        }
      }
    } catch { /* A missing index leaves the task title unknown. */ }
  }
  return { speed, task, logs }
}
