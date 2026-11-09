import { useEffect, useRef, useState, type CSSProperties } from 'react'
import type { TaskReading, Widget, WidgetValue } from './layout'
import { formatCountdown } from './status'

const taskLabels: Record<TaskReading['state'], string> = {
  running: '运行中',
  completed: '已完成',
  interrupted: '已中断',
  error: '运行异常',
  unknown: '状态未知',
}

export function WidgetReading({ widget, width, value }: { widget: Widget; width: number; value?: WidgetValue }) {
  const label = width === 1 ? widget.shortName : widget.name
  const number = typeof value === 'number' ? value : undefined
  const compactSpeed = widget.id === 'speed' && width === 1

  if (widget.kind === 'task') {
    const task = typeof value === 'object' ? value : undefined
    return (
      <>
        <span className="screen-label">{task ? taskLabels[task.state] : label}</span>
        <ScrollingTitle title={task ? task.title ?? '未命名对话' : '—'} />
      </>
    )
  }

  return (
    <>
      <span className="screen-label">{label}</span>
      {widget.kind === 'countdown' ? <Countdown deadline={number} /> : (
        <span className={`screen-reading${compactSpeed ? ' is-compact' : ''}`}>
          <strong>{number ?? '—'}</strong>{number !== undefined && <span>{widget.unit}</span>}
        </span>
      )}
      {widget.kind === 'progress' && (
        <span className="screen-meter" aria-hidden="true">
          <i style={{ width: `${Math.min(100, Math.max(0, number ?? 0))}%` }} />
        </span>
      )}
    </>
  )
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

  return <strong className="screen-countdown">{formatCountdown(deadline, now)}</strong>
}

function ScrollingTitle({ title }: { title: string }) {
  const viewport = useRef<HTMLSpanElement>(null)
  const text = useRef<HTMLSpanElement>(null)
  const [distance, setDistance] = useState(0)
  const gap = 18

  useEffect(() => {
    const box = viewport.current
    const line = text.current
    if (!box || !line) return
    const measure = () => setDistance(line.offsetWidth > box.clientWidth ? line.offsetWidth + gap : 0)
    const observer = new ResizeObserver(measure)
    observer.observe(box)
    observer.observe(line)
    measure()
    return () => observer.disconnect()
  }, [title])

  const style = distance > 0 ? {
    '--title-gap': `${gap}px`,
    '--title-travel': `${-distance}px`,
    '--title-duration': `${Math.max(4, distance / 38)}s`,
  } as CSSProperties : undefined

  return (
    <span className="screen-task-title" ref={viewport} title={title}>
      <span key={title} className={`screen-task-line${distance > 0 ? ' is-scrolling' : ''}`} style={style}>
        <span ref={text}>{title}</span>
        {distance > 0 && <span aria-hidden="true">{title}</span>}
      </span>
    </span>
  )
}
