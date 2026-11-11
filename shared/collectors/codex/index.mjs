import { defineModule } from '@mado/sdk'
import { localReadings } from './rollouts.mjs'

const metric = value => value == null ? null : { value }

const unsigned = value => Number.isSafeInteger(value) && value >= 0 ? value : null

async function initialize(context) {
  await context.process.request('appServer', 'initialize', {
    clientInfo: { name: 'tdisplay-collector', version: '1.0.0' },
  })
  await context.process.notify('appServer', 'initialized')
}

export async function readAccount(context, initialized = false) {
  if (!initialized) await initialize(context)
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

function outputs(limits, speed, task) {
  const { windows = [], resetCount } = limits ?? {}
  const window = windows.reduce((lowest, entry) =>
    lowest == null || entry.remainingPercent < lowest.remainingPercent ? entry : lowest, null)
  return {
    quota: window ? { value: window.remainingPercent, ratio: window.remainingPercent / 100 } : null,
    speed: metric(speed?.tokensPerSecond),
    resets: metric(resetCount),
    quotaReset: window?.resetsAt == null ? null : { deadlineUnixMs: window.resetsAt * 1000 },
    task: task ? { state: task.state, detail: task.title } : null,
  }
}

async function collect(_, context, initialized = false) {
  const previous = context.state ?? {}
  const now = context.sampledAtUnixMs
  let account = previous.account
  let checkedAt = previous.checkedAt
  if (account == null || !Number.isFinite(checkedAt) || now < checkedAt || now - checkedAt >= 120_000) {
    checkedAt = now
    try {
      account = await readAccount(context, initialized)
    } catch (error) {
      account = { login: 'error', message: String(error.message ?? error) }
    }
  }
  const { speed, task, logs } = await localReadings(context, previous.logs)
  const phase = { chatgpt: 'connected', missing: 'needsLogin', unsupported: 'unsupportedLogin' }[account.login] ?? 'error'
  const notify = task && previous.task?.id === task.id && previous.task.state === 'running'
    && ['completed', 'interrupted', 'error'].includes(task.state)
  return {
    outputs: outputs(account.limits, speed, task),
    notifications: notify ? ['task'] : [],
    state: { account, checkedAt, logs, task: task ? { id: task.id, state: task.state } : null },
    status: {
      phase,
      message: account.message ?? null,
      plan: account.plan ?? null,
      limits: account.limits ?? { windows: [], resetCount: null },
      accountUpdatedAt: phase === 'connected' ? Math.floor(checkedAt / 1000) : null,
      speed,
    },
  }
}

async function login(_, context) {
  await initialize(context)
  const { loginId, authUrl } = await context.process.request('appServer', 'account/login/start', {
    type: 'chatgpt',
  })
  if (typeof loginId !== 'string' || typeof authUrl !== 'string') {
    throw new Error('Codex 未返回完整的登录会话。')
  }
  await context.interaction.openUrl('loginBrowser', authUrl)
  for (let attempt = 0; attempt < 175; attempt++) {
    const event = await context.process.receive('appServer', 'account/login/completed')
    if (event?.loginId !== loginId) continue
    if (event.success !== true) throw new Error('浏览器登录未完成，请重试。')
    return collect({}, { ...context, state: null }, true)
  }
  throw new Error('等待浏览器登录超时。')
}

export default defineModule({ collect, handlers: { login } })
