import { Link2, Link2Off, PlugZap, X } from 'lucide-react'
import type { CSSProperties, KeyboardEvent, PointerEvent, Ref } from 'react'
import { display, gridHeight, widgets, type LayoutItem, type Snapshot } from './layout'
import { formatSyncTime } from './status'
import { WidgetReading } from './WidgetReading'

type ScreenProps = {
  layout: LayoutItem[]
  snapshot?: Snapshot
  gridRef?: Ref<HTMLDivElement>
  selected?: string
  dragging?: string
  target?: { item: LayoutItem; valid: boolean }
  onSelect?: (id: string) => void
  onRemove?: (id: string) => void
  onCardPointerDown?: (event: PointerEvent<HTMLButtonElement>, item: LayoutItem) => void
  onCardKeyDown?: (event: KeyboardEvent<HTMLButtonElement>, item: LayoutItem) => void
}

function placement(item: Pick<LayoutItem, 'x' | 'y' | 'w' | 'h'>): CSSProperties {
  return { gridColumn: `${item.x + 1} / span ${item.w}`, gridRow: `${item.y + 1} / span ${item.h}` }
}

export function Screen({
  layout, snapshot, gridRef, selected, dragging, target, onSelect, onRemove, onCardPointerDown, onCardKeyDown,
}: ScreenProps) {
  const geometry = {
    width: display.width,
    height: display.height,
    '--screen-pad': `${display.padding}px`,
    '--screen-gap': `${display.gap}px`,
    '--grid-height': `${gridHeight}px`,
    '--status-height': `${display.statusHeight}px`,
    '--columns': display.columns,
    '--rows': display.rows,
  } as CSSProperties

  return (
    <div className={`screen${onCardPointerDown ? ' is-editable' : ''}`} style={geometry} aria-label={`${display.width} × ${display.height} 显示画面`}>
      <div className="screen-grid" ref={gridRef}>
        {Array.from({ length: display.columns * display.rows }, (_, index) => (
          <div
            className="screen-cell"
            key={index}
            style={placement({ x: index % display.columns, y: Math.floor(index / display.columns), w: 1, h: 1 })}
            aria-hidden="true"
          />
        ))}
        {layout.map((item) => {
          const widget = widgets.get(item.id)
          if (!widget) return null
          const value = snapshot?.values[item.id]
          const body = <WidgetReading widget={widget} width={item.w} value={value} />
          const label = typeof value === 'object' && value.title ? `${widget.name}，${value.title}` : widget.name
          const style = placement(item)
          const className = `screen-widget${item.w === 1 ? ' is-narrow' : ''}${item.w === 2 && item.h === 2 ? ' is-large' : ''}${selected === item.id ? ' is-selected' : ''}${dragging === item.id ? ' is-dragging' : ''}`
          return (
            <div key={item.id} className={className} style={style} data-widget={item.id}>
              {onCardPointerDown ? (
                <>
                  <button
                    type="button"
                    aria-label={label}
                    aria-pressed={selected === item.id}
                    className="screen-card-control"
                    onClick={(event) => { if (event.detail === 0) onSelect?.(item.id) }}
                    onPointerDown={(event) => onCardPointerDown(event, item)}
                    onKeyDown={(event) => onCardKeyDown?.(event, item)}
                  >{body}</button>
                  {onRemove && (
                    <button type="button" className="card-remove" aria-label={`移除${widget.name}`}
                      title={`移除${widget.name}`} onClick={() => onRemove(item.id)}>
                      <X size={15} />
                    </button>
                  )}
                </>
              ) : body}
            </div>
          )
        })}
        {target && target.item.x >= 0 && target.item.y >= 0
          && target.item.x + target.item.w <= display.columns && target.item.y + target.item.h <= display.rows && (
          <div className={`screen-target ${target.valid ? 'is-valid' : 'is-invalid'}`} style={placement(target.item)} aria-hidden="true" />
        )}
      </div>
      <ScreenStatus snapshot={snapshot} />
    </div>
  )
}

function ScreenStatus({ snapshot }: { snapshot?: Snapshot }) {
  const ConnectionIcon = snapshot ? Link2 : Link2Off
  const connectionLabel = snapshot ? '设备已连接' : '设备未连接'
  const power = snapshot?.power
  const lastSync = formatSyncTime(snapshot?.lastSyncedAt)

  return (
    <div className="screen-status" role="group" aria-label="Status Bar">
      <span className="screen-link" role="img" aria-label={connectionLabel} title={connectionLabel}>
        <ConnectionIcon aria-hidden="true" />
      </span>
      <span className="screen-sync-time" title="上次同步时间">{lastSync}</span>
      <span className="screen-power" title={power?.source === 'battery' ? '电池电量' : undefined}>
        {power?.source === 'external'
          ? <PlugZap role="img" aria-label="直接供电" />
          : power?.source === 'battery' ? `${power.percent}%` : '—'}
      </span>
    </div>
  )
}
