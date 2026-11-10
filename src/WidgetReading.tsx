import { useEffect, useRef, useState, type CSSProperties } from 'react'
import type { TaskReading, Widget, WidgetValue } from './layout'
import { formatCountdown } from './status'
import { formatMetric, normalizedRatio } from './displayFormat'

const taskLabels: Record<TaskReading['state'], string> = {
  running: '运行中', completed: '已完成', interrupted: '已中断', error: '运行异常', unknown: '状态未知',
}
const taskColors: Record<string, string> = {
  running: '#397adc', completed: '#48b34a', interrupted: '#de7b3c', error: '#d63f41', unknown: '#9ba5b3',
}

export function WidgetReading({ widget, width, value }: { widget: Widget; width: number; value?: WidgetValue }) {
  const label = width === 1 ? widget.shortName : widget.name
  const reading = value && typeof value === 'object' && 'kind' in value ? value : undefined
  const number = typeof value === 'number' ? value : reading?.value
  const legacyTask = value && typeof value === 'object' && !('kind' in value) ? value : undefined
  const state = reading?.state ?? legacyTask?.state
  const isState = widget.kind === 'task' || widget.kind === 'state'
  const color = widget.accentColor ?? (widget.group === 'codex'
    ? isState && state ? taskColors[state] : widget.id === 'quota' ? '#48b34a' : widget.id === 'speed' ? '#397adc' : undefined
    : undefined) ?? 'var(--display-ink)'
  const face = useRef<HTMLSpanElement>(null)
  const animations = useRef<Animation[]>([])
  const previous = useRef<{ state?: string; reading: typeof reading } | undefined>(undefined)

  useEffect(() => {
    const before = previous.current
    previous.current = { state, reading }
    const element = face.current
    if (!element) return
    if (before?.state !== state) animations.current.forEach(animation => animation.cancel())
    // Snapshots on mount are static. Ordinary refreshes preserve the current animation.
    if (!before || !isState || !reading?.notify || before.reading === reading
      || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return
    animations.current.forEach(animation => animation.cancel())
    const style = getComputedStyle(element)
    const surface = style.getPropertyValue('--display-surface').trim()
    const ink = style.getPropertyValue('--display-ink').trim()
    const accent = color.startsWith('var(') ? ink : color
    const rgb = /^#([0-9a-f]{6})$/i.exec(accent)?.[1].match(/../g)?.map(hex => parseInt(hex, 16) / 255)
    const linear = rgb?.map(channel => channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4)
    const alertInk = linear && linear[0] * .2126 + linear[1] * .7152 + linear[2] * .0722 > .179 ? '#000000' : '#ffffff'
    const timing = { duration: 1000, iterations: 2 }
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
  }, [state, reading, color, isState])
  useEffect(() => () => animations.current.forEach(animation => animation.cancel()), [])

  const ratio = normalizedRatio(reading ? reading.ratio ?? 0 : (number ?? 0) / 100)
  const style = { '--widget-accent': color, '--progress-width': `${Math.round((79 * width - 4) * ratio)}px` } as CSSProperties
  const title = reading?.detail ?? legacyTask?.title
  const labelText = isState && state ? widget.stateLabels?.[state] ?? taskLabels[state as TaskReading['state']] ?? state : label
  const hasValue = number != null && Number.isFinite(number)
  const delta = reading?.delta
  const bounds = reading?.bounds

  return (
    <span className="widget-face" data-kind={isState ? 'state' : widget.kind} ref={face} style={style}>
      <span className={`screen-bar${widget.kind === 'progress' ? ' is-progress' : ''}`} aria-hidden="true">
        {widget.kind === 'progress' && <i />}
      </span>
      <ScrollingLine className="screen-label" text={labelText} />
      {isState ? <ScrollingLine className="screen-task-title" text={title ?? (state && widget.group === 'codex' ? '未命名对话' : '—')} />
        : widget.kind === 'chart' ? <Chart samples={reading?.values ?? []} min={reading?.min ?? 0} max={reading?.max ?? 100} unit={widget.unit} />
        : widget.kind === 'countdown' || reading?.deadlineUnixMs !== undefined
          ? <Countdown deadline={reading?.deadlineUnixMs === undefined ? number : reading.deadlineUnixMs / 1000} />
          : <span className="screen-reading">
            <MetricValue value={number} />
            {hasValue && widget.unit && <span className="screen-unit">{widget.unit}</span>}
            {widget.kind === 'trend' && delta != null && Number.isFinite(delta) && <small className="screen-delta" data-direction={Math.sign(delta)} title={`${delta > 0 ? '+' : ''}${delta}${reading?.deltaUnit ?? ''}`}>
              {delta > 0 ? '↑' : delta < 0 ? '↓' : '→'}{formatMetric(Math.abs(delta))}{reading?.deltaUnit === 'pp' ? '%' : reading?.deltaUnit ?? ''}
            </small>}
          </span>}
      {widget.kind === 'range' && <span className="screen-range" aria-hidden="true">
        <i className="rail" /><i className="tick" /><i className="tick" /><i className="tick" />
        {bounds && hasValue && <i className="marker" style={{ left: `${normalizedRatio(number, bounds.min, bounds.max) * 100}%` }} />}
      </span>}
    </span>
  )
}

function Chart({ samples, min, max, unit }: { samples: (number | null)[]; min: number; max: number; unit: string }) {
  const points = samples.slice(-12)
  const latest = points.at(-1)
  const columns = [...Array<null>(12 - points.length).fill(null), ...points]
  return <>
    <svg className="screen-chart" viewBox="0 0 59 27" preserveAspectRatio="none" role="img" aria-label={latest == null ? '暂无读数' : `${latest}${unit}`}>
      {columns.flatMap((sample, column) => Array.from({ length: sample == null ? 0 : Math.round(normalizedRatio(sample, min, max) * 7) }, (_, row) =>
        <rect key={`${column}-${row}`} x={column * 5} y={24 - row * 4} width={4} height={3} />))}
    </svg>
    <small className="screen-chart-value">{formatMetric(latest)}{latest != null && Number.isFinite(latest) ? unit : ''}</small>
  </>
}

function MetricValue({ value }: { value?: number }) {
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
  return <strong className={`screen-value${text.length >= 4 ? ' is-four-char' : ''}`}>{text}</strong>
}

function Countdown({ deadline }: { deadline?: number }) {
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
  const text = formatCountdown(deadline, now)
  return <strong className={`screen-countdown${text.length >= 4 || text === '待刷新' ? ' is-compact' : ''}`}>{text}</strong>
}

/** Only the overflowing text line moves; the mask follows its current card background. */
function ScrollingLine({ text, className }: { text: string; className: string }) {
  const normalized = text.replace(/[\r\n\t]+/g, ' ')
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
      delay: 800, duration: travel * 1000 / 32, iterations: Infinity,
    })
    const timer = window.setTimeout(() => box.classList.remove('is-holding'), 800)
    return () => { animation.cancel(); window.clearTimeout(timer); box.classList.remove('is-holding') }
  }, [normalized, travel])
  return <span className={`${className} screen-line${travel ? ' is-overflowing' : ''}`} ref={viewport} title={normalized}>
    <span className="screen-line-track" ref={track}>
      <span ref={content}>{normalized}</span>{travel > 0 && <span aria-hidden="true">{normalized}</span>}
    </span>
  </span>
}
