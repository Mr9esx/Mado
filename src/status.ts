export type PowerStatus = { source: 'external' } | { source: 'battery'; percent: number }

function twoDigits(value: number) {
  return String(value).padStart(2, '0')
}

export function formatSyncTime(seconds?: number | null) {
  if (seconds == null) return '—'
  const date = new Date(seconds * 1000)
  const year = String(date.getFullYear()).padStart(4, '0')
  const day = [date.getMonth() + 1, date.getDate()].map(twoDigits).join(':')
  const time = [date.getHours(), date.getMinutes(), date.getSeconds()].map(twoDigits).join(':')
  return `${year}:${day} ${time}`
}

export function formatCountdown(deadline: number | undefined, now: number, expiredText = '待刷新') {
  if (deadline === undefined || !Number.isFinite(deadline)) return '—'
  const seconds = Math.ceil(deadline - now)
  if (seconds <= 0) return expiredText
  if (seconds >= 86400) return `${Math.floor(seconds / 86400)}d${Math.floor(seconds / 3600) % 24}h`
  if (seconds >= 3600) return `${Math.floor(seconds / 3600)}h${twoDigits(Math.floor(seconds / 60) % 60)}m`
  if (seconds >= 60) return `${Math.floor(seconds / 60)}m`
  return `${seconds}s`
}
