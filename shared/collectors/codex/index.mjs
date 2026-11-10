import { defineModule } from '@tdisplay/sdk'

const metric = value => value == null ? null : { value }

const unsigned = value => Number.isSafeInteger(value) && value >= 0 ? value : null

export async function readAccount(_, context) {
  const { account } = await context.process.request('appServer', 'account/read', { refreshToken: false })
  if (account == null) return { login: 'missing' }
  if (account.type !== 'chatgpt') return { login: 'unsupported' }

  const response = await context.process.request('appServer', 'account/rateLimits/read')
  const specific = response.rateLimitsByLimitId?.codex
  const limits = specific && typeof specific === 'object' && !Array.isArray(specific) ? specific : response.rateLimits
  const windows = ['primary', 'secondary'].map(key => limits?.[key])
    .filter(window => Number.isFinite(window?.usedPercent))
    .map(window => ({
      durationMinutes: unsigned(window.windowDurationMins),
      remainingPercent: Math.round(Math.max(0, Math.min(100, 100 - window.usedPercent))),
      resetsAt: unsigned(window.resetsAt),
    }))
    .sort((a, b) => (a.durationMinutes ?? Infinity) - (b.durationMinutes ?? Infinity))
  return {
    login: 'chatgpt',
    plan: typeof account.planType === 'string' ? account.planType : null,
    limits: { windows, resetCount: unsigned(response.rateLimitResetCredits?.availableCount) },
  }
}

export default defineModule({
  async collect(settings, context) {
    const account = settings.mode === 'account' ? await readAccount(settings, context) : null
    const { windows = [], resetCount } = account?.limits ?? settings.limits ?? {}
    const { speed, task } = settings
    const limitingWindow = windows.reduce((lowest, window) =>
      lowest == null || window.remainingPercent < lowest.remainingPercent ? window : lowest, null)
    const quota = limitingWindow?.remainingPercent
    const resetSeconds = limitingWindow?.resetsAt
    return {
      outputs: {
        quota: quota == null ? null : { value: quota, ratio: quota / 100 },
        speed: metric(speed),
        resets: metric(resetCount),
        quotaReset: resetSeconds == null ? null : { deadlineUnixMs: resetSeconds * 1000 },
        task: task == null ? null : { state: task.state, detail: task.title },
      },
    }
  },
})

export function summarizeRollouts({ samples, turns, nowMicros }) {
  const order = (a, b) => b.endedMs - a.endedMs || b.durationMs - a.durationMs || b.tokens - a.tokens
  const unique = samples.sort(order)
    .filter((sample, i) => i === 0 || order(sample, samples[i - 1]) !== 0)
    .slice(0, 10)
  const totalTokens = unique.reduce((sum, sample) => sum + sample.tokens, 0)
  const totalMs = unique.reduce((sum, sample) => sum + sample.durationMs, 0)
  const speed = unique.length === 0 ? null : {
    tokensPerSecond: Math.round(totalTokens / (totalMs / 1000)),
    samples: unique.length,
    latestAt: Math.trunc(unique[0].endedMs / 1000),
  }

  const candidates = turns.map(({ id, state, modified, started }) => {
    const running = state === 'running' && nowMicros - modified <= 180_000_000
    return { id, running, time: running ? started ?? modified : modified,
      state: state === 'running' && !running ? 'unknown' : state }
  })
  candidates.sort((a, b) => Number(b.running) - Number(a.running) || b.time - a.time ||
    (a.id < b.id ? 1 : a.id > b.id ? -1 : 0))
  const task = candidates[0]
  return { speed, task: task ? { id: task.id, state: task.state } : null }
}
