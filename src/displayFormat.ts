/** The device reserves at most four characters for a numeric reading. */
export function formatMetric(value?: number | null): string {
  if (value == null || !Number.isFinite(value)) return '—'
  if (Math.abs(value) < 1000) {
    for (let precision = 3; precision >= 0; precision--) {
      const rounded = Number(value.toFixed(precision))
      if (Math.abs(rounded) >= 1000) break
      const text = String(rounded)
      if (text.length <= 4) return text
    }
  }
  for (let precision = 2; precision >= 1; precision--) {
    const text = new Intl.NumberFormat('en', { notation: 'compact', maximumSignificantDigits: precision })
      .format(value).toLowerCase()
    if (text.length <= 4) return text
  }
  const scientific = value.toExponential(0).replace('e+', 'e')
  return scientific.length <= 4 ? scientific : '超限'
}

export function normalizedRatio(value: number, min = 0, max = 1) {
  return Number.isFinite(value) && Number.isFinite(min) && Number.isFinite(max) && max > min
    ? Math.min(1, Math.max(0, (value - min) / (max - min))) : 0
}
