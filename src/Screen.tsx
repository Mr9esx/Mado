import { Monitor, X } from 'lucide-react'
import { version } from '../package.json'
import type { CSSProperties, KeyboardEvent, PointerEvent, Ref } from 'react'
import { display, gridHeight, widgetCatalog, type DisplayPage, type LayoutItem, type Snapshot } from './layout'
import { formatSyncTime } from './status'
import { WidgetReading } from './WidgetReading'
import { useAppearance } from './appearance'
import { DeviceIcon } from './DeviceIcon'

type ScreenProps = {
  pages?: DisplayPage[]
  pageId?: string
  onPageChange?: () => void
  layout: LayoutItem[]
  snapshot?: Snapshot
  theme?: 'dark' | 'light'
  surface?: 'device' | 'desktop'
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
  layout, snapshot, pages, pageId, onPageChange, theme, surface = 'device', gridRef, selected, dragging, target, onSelect, onRemove, onCardPointerDown, onCardKeyDown,
}: ScreenProps) {
  const { appearance } = useAppearance()
  const widgets = widgetCatalog(snapshot)
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
  const message = surface === 'desktop' ? (!snapshot ? '正在加载' : undefined)
    : onCardPointerDown ? undefined : !snapshot ? '等待连接' : snapshot.lastSyncedAt == null ? '等待同步' : undefined

  return (
    <div className={`screen${onCardPointerDown ? ' is-editable' : ''}`} data-display-theme={theme ?? appearance.displayTheme} style={geometry} aria-label={`${display.width} × ${display.height} 显示画面`}>
      {message ? <div className="screen-message">{message}</div> : <div className="screen-grid" ref={gridRef}>
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
          const detail = typeof value === 'object' ? ('kind' in value ? value.detail : value.title) : null
          const label = detail ? `${widget.name}，${detail}` : widget.name
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
      </div>}
      <ScreenStatus snapshot={snapshot} surface={surface} pages={pages ?? snapshot?.pages}
        pageId={pageId ?? snapshot?.activePageId} onPageChange={onPageChange} />
    </div>
  )
}

function ScreenStatus({ snapshot, pages, pageId, onPageChange, surface }: { snapshot?: Snapshot; pages?: DisplayPage[]; pageId?: string; onPageChange?: () => void; surface: 'device' | 'desktop' }) {
  const index = Math.max(0, pages?.findIndex(page => page.id === pageId) ?? 0)
  const pageName = pages?.[index]?.name ?? 'Codex 软件'
  const PageGroup = onPageChange ? 'button' : 'span'
  const power = snapshot?.power
  const lastSync = formatSyncTime(snapshot?.lastSyncedAt).split(' ').at(-1)

  return (
    <div className="screen-status" role="group" aria-label="Status Bar">
      <PageGroup className="screen-page-group" onClick={onPageChange} title={onPageChange ? '下一页' : pageName} aria-label={onPageChange ? `${pageName}，第 ${index + 1} 页，共 ${pages?.length ?? 1} 页，点击下一页` : undefined}>
        <span className="screen-page-dots" role="img" aria-label={`第 ${index + 1} 页，共 ${pages?.length ?? 1} 页`}>{Array.from({ length: pages?.length ?? 1 }, (_, i) => <i key={i} data-active={i === index} />)}</span>
        <span className="screen-page">{pageName}</span>
      </PageGroup>
      <span className="screen-info">
        <span title={surface === 'desktop' ? '最近应用布局的时间' : '上次成功同步时间'}>Updated at {lastSync}</span>
        {surface === 'desktop' ? <span title="TDisplay Studio 应用版本">App v{version}</span>
          : <span title="模拟设备尚未提供固件版本">FW —</span>}
      </span>
      <span className="screen-status-right">
        {surface === 'desktop' ? <><Monitor size={14} role="img" aria-label="本机数据" /><span>本机</span></> : <>
        <DeviceIcon name={snapshot ? 'usb' : 'loading-four'} size={14} label={snapshot ? '模拟设备已连接' : '等待连接'} />
        <span className="screen-power">
          {power?.source === 'external' ? <DeviceIcon name="plug-one" size={18} label="外部供电" />
            : power?.source === 'battery' ? <><DeviceIcon name="battery-empty" size={18} label="电池电量" charge={power.percent} /><span>{power.percent}%</span></>
              : <span aria-label="供电来源未知">—</span>}
        </span>
        </>}
      </span>
    </div>
  )
}
