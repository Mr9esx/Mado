import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
import type { NumberAnimation, TaskReading, Widget, WidgetValue } from './layout'
import { formatCountdown } from './status'
import { digitRollColumns, formatMetric, normalizedRatio } from './displayFormat'

const taskLabels: Record<TaskReading['state'], string> = {
  running: '运行中', completed: '已完成', interrupted: '已中断', error: '运行异常', unknown: '状态未知',
}
export function WidgetReading({ widget, width, height = 1, value, sequence }: {
  widget: Widget; width: number; height?: number; value?: WidgetValue | null; sequence?: number
}) {
  const label = width === 1 ? widget.shortName : widget.name
  const reading = value && typeof value === 'object' && 'kind' in value ? value : undefined
  const number = typeof value === 'number' ? value : reading?.value
  const legacyTask = value && typeof value === 'object' && !('kind' in value) ? value : undefined
  const state = reading?.state ?? legacyTask?.state
  const isState = widget.kind === 'task' || widget.kind === 'state'
  const stateStyle = isState && state ? widget.stateStyles?.[state] : undefined
  const color = stateStyle?.color ?? widget.accentColor ?? 'var(--display-ink)'
  const alertDuration = stateStyle?.alertDurationMs ?? 2000
  const alertText = stateStyle?.alertText ?? 'auto'
  const face = useRef<HTMLSpanElement>(null)
  const animations = useRef<Animation[]>([])
  const previous = useRef<{ state?: string; reading: typeof reading; sequence?: number } | undefined>(undefined)

  useEffect(() => {
    const before = previous.current
    previous.current = { state, reading, sequence }
    const element = face.current
    if (!element) return
    if (before?.state !== state) animations.current.forEach(animation => animation.cancel())
    // Snapshots on mount are static. Ordinary refreshes preserve the current animation.
    if (!before || !isState || !widget.supportsNotify || !reading?.notify || !alertDuration
      || (sequence === undefined ? before.reading === reading : before.sequence === sequence)
      || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    animations.current.forEach(animation => animation.cancel())
    const style = getComputedStyle(element)
    const surface = style.getPropertyValue('--display-surface').trim()
    const ink = style.getPropertyValue('--display-ink').trim()
    const accent = color.startsWith('var(') ? ink : color
    const rgb = /^#([0-9a-f]{6})$/i.exec(accent)?.[1].match(/../g)?.map(hex => parseInt(hex, 16) / 255)
    const linear = rgb?.map(channel => channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4)
    const alertInk = alertText !== 'auto' ? alertText
      : linear && linear[0] * .2126 + linear[1] * .7152 + linear[2] * .0722 > .179 ? '#000000' : '#ffffff'
    const timing = { duration: 1000, iterations: alertDuration / 1000 }
    const easing = 'ease-in-out'
    animations.current = [
      element.animate([
        { backgroundColor: surface, offset: 0, easing },
        { backgroundColor: accent, offset: .42, easing },
        { backgroundColor: surface, offset: 1 },
      ], timing),
      element.animate([
        { color: ink, offset: 0 }, { color: ink, offset: .12, easing },
        { color: alertInk, offset: .42 }, { color: alertInk, offset: .54, easing },
        { color: ink, offset: 1 },
      ], timing),
    ]
  }, [state, reading, sequence, color, isState, alertDuration, alertText, widget.supportsNotify])
  useEffect(() => () => animations.current.forEach(animation => animation.cancel()), [])

  const ratio = normalizedRatio(reading ? reading.ratio ?? 0 : (number ?? 0) / 100)
  const style = { '--widget-accent': color, '--progress-width': `${Math.round((79 * width - 4) * ratio)}px` } as CSSProperties
  const title = reading?.detail ?? legacyTask?.title
  const labelText = isState && state ? widget.stateLabels?.[state] ?? taskLabels[state as TaskReading['state']] ?? state : label
  const hasValue = number != null && Number.isFinite(number)
  const delta = reading?.delta
  const range = widget.range
  const bounds = range?.mode === 'fixed' && range.min != null && range.max != null
    ? { min: range.min, max: range.max } : reading?.bounds
  const hasBounds = bounds && Number.isFinite(bounds.min) && Number.isFinite(bounds.max) && bounds.min < bounds.max
  const markerRatio = bounds && hasValue ? normalizedRatio(number, bounds.min, bounds.max) : undefined
  const details = height === 2 ? (widget.detailFields ?? []).flatMap(field => {
    const detail = reading?.details?.[field.key]
    return detail ? [{ key: field.key, text: `${field.label} ${detail}` }] : []
  }) : []
  const deltaText = delta == null ? '—' : formatMetric(Math.abs(delta))
  const deltaDirection = deltaText === '0' ? 0 : Math.sign(delta ?? 0)
  const deltaUnit = reading?.deltaUnit ?? widget.trend?.deltaUnit ?? widget.unit

  return (
    <span className="widget-face" data-kind={isState ? 'state' : widget.kind} data-height={height}
      data-has-details={details.length > 0} data-has-bounds={!!hasBounds} ref={face} style={style}>
      <span className={`screen-bar${widget.kind === 'progress' ? ' is-progress' : ''}`} aria-hidden="true">
        {widget.kind === 'progress' && <i />}
      </span>
      <ScrollingLine className="screen-label" text={labelText} speed={16} titleTracking />
      {isState ? <ScrollingLine className="screen-task-title" text={title ?? (state && widget.group === 'codex' ? '未命名对话' : '—')} />
        : widget.kind === 'chart' ? <Chart samples={reading?.values ?? []} sampleCount={widget.chart?.sampleCount ?? 12}
          min={widget.chart?.min ?? reading?.min ?? 0}
          max={widget.chart?.max ?? reading?.max ?? 100} unit={widget.unit} />
        : widget.format === 'countdown' || widget.kind === 'countdown' || reading?.deadlineUnixMs !== undefined
          ? <Countdown deadline={reading?.deadlineUnixMs === undefined ? number : reading.deadlineUnixMs / 1000}
            expiredText={widget.expiredText} />
          : <span className="screen-reading">
            <MetricValue value={number} animation={widget.numberAnimation ?? 'none'} />
            {hasValue && widget.unit && <span className="screen-unit" data-long={widget.unit.length > 2 || undefined}>{widget.unit}</span>}
            {widget.kind === 'trend' && delta != null && Number.isFinite(delta) && <small className="screen-delta" data-direction={deltaDirection} title={`${delta > 0 ? '+' : ''}${delta}${deltaUnit}`}>
              {deltaDirection > 0 ? '↑' : deltaDirection < 0 ? '↓' : '→'}{deltaText}{deltaUnit === 'pp' ? '%' : deltaUnit}
            </small>}
          </span>}
      {widget.kind === 'range' && hasBounds && <span className="screen-range" aria-hidden="true">
        <i className="rail" /><i className="tick" /><i className="tick" /><i className="tick" />
        {markerRatio !== undefined && <i className="marker" style={{ left: `calc(${markerRatio * 100}% - ${markerRatio * 3}px)` }} />}
      </span>}
      {details.length > 0 && <span className="screen-details">{details.map(detail =>
        <ScrollingLine key={detail.key} className="screen-detail" text={detail.text} />)}</span>}
    </span>
  )
}

function Chart({ samples, sampleCount, min, max, unit }: { samples: (number | null)[]; sampleCount: number; min: number; max: number; unit: string }) {
  const points = samples.slice(-sampleCount)
  const latest = points.at(-1)
  const columns = [...Array<null>(sampleCount - points.length).fill(null), ...points]
  return <>
    <svg className="screen-chart" viewBox={`0 0 ${sampleCount * 5 - 1} 27`} preserveAspectRatio="none" role="img" aria-label={latest == null ? '暂无读数' : `${latest}${unit}`}>
      {columns.flatMap((sample, column) => Array.from({ length: sample == null ? 0 : Math.round(normalizedRatio(sample, min, max) * 7) }, (_, row) =>
        <rect key={`${column}-${row}`} x={column * 5} y={24 - row * 4} width={4} height={3} />))}
    </svg>
    <small className="screen-chart-value">{formatMetric(latest)}{latest != null && Number.isFinite(latest) ? unit : ''}</small>
  </>
}

function MetricValue({ value, animation }: { value?: number; animation: NumberAnimation }) {
  if (animation === 'count') return <CountingValue value={value} />
  if (animation === 'digitRoll') return <RollingValue value={value} />
  return <ValueText text={formatMetric(value)} />
}

function ValueText({ text, children }: { text: string; children?: React.ReactNode }) {
  return <strong className={`screen-value${text.length >= 4 ? ' is-four-char' : ''}`} aria-label={children ? text : undefined}>
    {children ?? text}
  </strong>
}

function RollingValue({ value }: { value?: number }) {
  const previous = useRef(value)
  const [roll, setRoll] = useState<{ target: number; columns: string[][]; key: number }>()
  useLayoutEffect(() => {
    const start = previous.current
    previous.current = value
    if (start == null || value == null || !Number.isFinite(start) || !Number.isFinite(value)
      || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      setRoll(undefined)
      return
    }
    if (start === value) return
    setRoll({ target: value, columns: digitRollColumns(formatMetric(start), formatMetric(value), value > start ? 1 : -1), key: performance.now() })
  }, [value])
  const text = formatMetric(value)
  if (roll?.target !== value || !roll) return <ValueText text={text} />
  return <ValueText text={text}><span className="screen-digits" key={roll.key} aria-hidden="true">
    {roll.columns.map((column, index) => column.length === 1 ? <span key={index}>{column[0]}</span>
      : <span key={index} className="screen-digit-window"><span className="screen-digit-strip">
        {column.map((digit, row) => <span key={row}>{digit}</span>)}
      </span></span>)}
  </span></ValueText>
}

function CountingValue({ value }: { value?: number }) {
  const [shown, setShown] = useState(value)
  const current = useRef(value)
  useEffect(() => {
    const start = current.current
    if (start == null || value == null || !Number.isFinite(start) || !Number.isFinite(value)
      || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      current.current = value
      setShown(value)
      return
    }
    if (start === value) return
    const began = performance.now()
    const duration = Math.min(800, Math.max(300, Math.abs(value - start) * 40))
    const precision = Number.isInteger(start) && Number.isInteger(value) ? 0 : 2
    let frame: number
    const tick = (now: number) => {
      const progress = Math.min(1, (now - began) / duration)
      const next = progress === 1 ? value : Number((start + (value - start) * progress).toFixed(precision))
      current.current = next
      setShown(next)
      if (progress < 1) frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [value])
  const text = formatMetric(shown)
  return <ValueText text={text} />
}

function Countdown({ deadline, expiredText = '待刷新' }: { deadline?: number; expiredText?: string }) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    if (deadline === undefined) return
    let timer: number | undefined
    const tick = () => {
      const timestamp = Date.now()
      setNow(timestamp / 1000)
      if (timestamp / 1000 < deadline) timer = window.setTimeout(tick, 1000 - timestamp % 1000)
    }
    tick()
    return () => window.clearTimeout(timer)
  }, [deadline])
  const text = formatCountdown(deadline, now, expiredText)
  return <strong className={`screen-countdown${text.length >= 4 || text === expiredText ? ' is-compact' : ''}`} title={text}>{text}</strong>
}

// Match the Han ideograph ranges used by the firmware's title tracking.
const hanRuns = /([\u3007\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF\u{20000}-\u{2FA1F}\u{30000}-\u{3347F}]+)/u

/** Only the overflowing text line moves; the mask follows its current card background. */
function ScrollingLine({ text, className, speed = 32, titleTracking = false }: {
  text: string; className: string; speed?: number; titleTracking?: boolean
}) {
  const normalized = text.replace(/[\r\n\t]+/g, ' ')
  const formatted = titleTracking ? normalized.split(hanRuns).map((run, index) => (
    index % 2 ? <span className="screen-label-han" key={index}>{run}</span> : run
  )) : normalized
  const viewport = useRef<HTMLSpanElement>(null)
  const track = useRef<HTMLSpanElement>(null)
  const content = useRef<HTMLSpanElement>(null)
  const [travel, setTravel] = useState(0)
  useEffect(() => {
    const box = viewport.current
    const line = content.current
    if (!box || !line) return
    const measure = () => setTravel(line.offsetWidth > box.clientWidth ? line.offsetWidth + 18 : 0)
    const observer = new ResizeObserver(measure)
    observer.observe(box)
    observer.observe(line)
    measure()
    return () => observer.disconnect()
  }, [normalized])
  useEffect(() => {
    const box = viewport.current
    const line = track.current
    if (!box || !line || !travel) return
    box.classList.add('is-holding')
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    const animation = line.animate([{ transform: 'translateX(0)' }, { transform: `translateX(-${travel}px)` }], {
      delay: 800, duration: travel * 1000 / speed, iterations: Infinity,
    })
    const timer = window.setTimeout(() => box.classList.remove('is-holding'), 800)
    return () => { animation.cancel(); window.clearTimeout(timer); box.classList.remove('is-holding') }
  }, [normalized, speed, travel])
  return <span className={`${className} screen-line${travel ? ' is-overflowing' : ''}`} ref={viewport} title={normalized}>
    <span className="screen-line-track" ref={track}>
      <span ref={content}>{formatted}</span>{travel > 0 && <span aria-hidden="true">{formatted}</span>}
    </span>
  </span>
}
