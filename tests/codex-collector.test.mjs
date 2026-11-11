import assert from 'node:assert/strict'
import test from 'node:test'
import { parseRollout, summarizeRollouts, threadId } from '../shared/collectors/codex/rollouts.mjs'

const line = (type, payload, seconds = 0) => JSON.stringify({
  type, payload, timestamp: new Date(Date.UTC(2026, 10, 9, 1, 0, seconds)).toISOString(),
})
const usage = (total, last, seconds) => line('event_msg', {
  type: 'token_count',
  info: { total_token_usage: { output_tokens: total }, last_token_usage: { output_tokens: last } },
}, seconds)

test('MJS parses response speed without counting tool runtime or duplicate token events', () => {
  const log = [
    line('event_msg', { type: 'task_started' }),
    line('response_item', { type: 'custom_tool_call' }, 2),
    line('response_item', { type: 'custom_tool_call_output' }, 12),
    usage(100, 100, 12),
    line('response_item', { type: 'message', role: 'assistant' }, 15),
    usage(160, 60, 15),
    usage(160, 60, 16),
  ].join('\n')
  const { samples } = parseRollout(log)
  const { speed } = summarizeRollouts({ samples: [...samples, ...samples], turns: [], nowMs: 0 })
  assert.equal(speed.samples, 2)
  assert.equal(speed.tokensPerSecond, 32)
})

test('MJS selects a live root turn by start time and ages stale tasks', () => {
  const root = line('session_meta', { id: 'root' })
  const child = line('session_meta', { id: 'child', parent_thread_id: 'root' })
  assert.equal(threadId(root), 'root')
  assert.equal(threadId(child), null)
  const { turn } = parseRollout([
    line('event_msg', { type: 'task_started', turn_id: 'new', started_at: 200 }),
    line('event_msg', { type: 'task_complete', turn_id: 'old' }),
  ].join('\n'))
  const turns = [
    { ...turn, id: 'root', modified: 210_000 },
    { state: 'running', id: 'older', started: 100_000, modified: 220_000 },
  ]
  assert.deepEqual(summarizeRollouts({ samples: [], turns, nowMs: 230_000 }).task, {
    id: 'root', state: 'running',
  })
  assert.equal(summarizeRollouts({ samples: [], turns, nowMs: 410_000 }).task.state, 'unknown')
})
