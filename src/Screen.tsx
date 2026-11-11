import { Monitor, X } from 'lucide-react'
import { version } from '../package.json'
import { useEffect, useRef, type CSSProperties, type KeyboardEvent, type PointerEvent, type Ref } from 'react'
import { cardWidget, display, gridHeight, widgetCatalog, type DisplayPage, type DisplayTheme, type LayoutItem, type Snapshot } from './layout'
import { formatSyncTime } from './status'
import { WidgetReading } from './WidgetReading'
import { useAppearance } from './appearance'
import { DeviceIcon } from './DeviceIcon'
import type { CardPlaybackFrame } from './useCardPlayback'

export type ScreenConnection = {
  phase: 'waiting' | 'syncing' | 'connected' | 'disconnected' | 'failed'
  transport?: 'usb' | 'wifi' | 'ble'
  firmwareVersion?: string
  lastReadingsAt?: number | null
  reason?: string
  power?: { source: 'external' | 'unknown' } | { source: 'battery'; percent: number | null; charging?: boolean }
  hasLayout?: boolean
}

type ScreenProps = {
  pages?: DisplayPage[]
  pageId?: string
  onPageChange?: () => void
  layout: LayoutItem[]
  snapshot?: Snapshot
  theme?: 'dark' | 'light'
  palette?: DisplayTheme
  connection?: ScreenConnection
  surface?: 'device' | 'desktop'
  gridRef?: Ref<HTMLDivElement>
  selected?: string
  dragging?: string
  playback?: CardPlaybackFrame & { widgetId: string }
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
  layout, snapshot, pages, pageId, onPageChange, theme, palette, connection, surface = 'device', gridRef, selected, dragging, playback, target, onSelect, onRemove, onCardPointerDown, onCardKeyDown,
}: ScreenProps) {
  const { appearance } = useAppearance()
  const widgets = widgetCatalog(snapshot)
  const frame = !onCardPointerDown ? snapshot?.displayFrame : undefined
  const pageFrame = frame?.config.pages.find(page => page.pageId === (pageId ?? snapshot?.activePageId))
  const configuredTheme = palette ?? frame?.config.theme ?? snapshot?.theme
  const themeName = theme ?? configuredTheme?.active ?? appearance.displayTheme
  const colors = configuredTheme?.[themeName]
  const geometry = {
    width: display.width,
    height: display.height,
    '--screen-pad': `${display.padding}px`,
    '--screen-gap': `${display.gap}px`,
    '--grid-height': `${gridHeight}px`,
    '--status-height': `${display.statusHeight}px`,
    '--columns': display.columns,
    '--rows': display.rows,
    ...(colors && {
      '--display-screen': colors.background,
      '--display-surface': colors.surface,
      '--display-ink': colors.text,
    }),
  } as CSSProperties
  const phase = connection?.phase ?? (!snapshot ? 'waiting' : snapshot.lastSyncedAt == null ? 'syncing' : 'connected')
  const hasLayout = connection?.hasLayout ?? (snapshot?.lastSyncedAt != null)
  const connectionFailed = surface === 'device' && !onCardPointerDown && !hasLayout && phase === 'failed'
  const message = surface === 'desktop' ? (!snapshot ? '正在加载' : undefined)
    : onCardPointerDown || hasLayout ? undefined : phase === 'syncing' || phase === 'connected' ? '等待同步' : '等待连接'

  return (
    <div className={`screen${onCardPointerDown ? ' is-editable' : ''}`} data-display-theme={themeName} style={geometry} aria-label={`${display.width} × ${display.height} 显示画面`}>
      {connectionFailed ? <ConnectionFailure reason={connection?.reason ?? '连接超时，请检查电脑端服务及 USB 数据线。'} />
        : message ? <div className="screen-message">{message}</div> : <div className="screen-grid" ref={gridRef}>
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
          const card = pageFrame?.cards.find(candidate => frame?.sourceIds[candidate.cardId] === item.id)
          const trial = playback?.widgetId === item.id ? playback : undefined
          const value = trial ? trial.value : card ? frame?.readings[card.cardId] : snapshot?.values[item.id]
          const definition = card
            ? { ...widget, ...card.render, name: card.render.label, shortName: card.render.label }
            : cardWidget(widget, item.appearance)
          const body = <WidgetReading key={trial?.playId ?? 'live'} widget={definition} width={item.w} height={item.h}
            value={value} sequence={trial ? undefined : snapshot?.sequence} />
          const detail = value && typeof value === 'object' ? ('kind' in value ? value.detail : value.title) : null
          const label = detail ? `${widget.name}，${detail}` : widget.name
          const style = placement(item)
          const className = `screen-widget${item.w === 1 ? ' is-narrow' : ''}${item.w === 2 && item.h === 2 ? ' is-large' : ''}${selected === item.id ? ' is-selected' : ''}${dragging === item.id ? ' is-dragging' : ''}`
          return (
            <div key={`${pageId ?? snapshot?.activePageId ?? ''}:${item.id}`} className={className} style={style} data-widget={item.id}>
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
      <ScreenStatus snapshot={snapshot} surface={surface} pages={pages ?? snapshot?.pages} phase={phase} connection={connection}
        pageId={pageId ?? snapshot?.activePageId} onPageChange={onPageChange} />
    </div>
  )
}

function ScreenStatus({ snapshot, pages, pageId, onPageChange, surface, phase, connection }: {
  snapshot?: Snapshot; pages?: DisplayPage[]; pageId?: string; onPageChange?: () => void; surface: 'device' | 'desktop'
  phase: ScreenConnection['phase']; connection?: ScreenConnection
}) {
  const index = Math.max(0, pages?.findIndex(page => page.id === pageId) ?? 0)
  const pageName = pages?.[index]?.name ?? 'Codex 软件'
  const PageGroup = onPageChange ? 'button' : 'span'
  const power = connection ? connection.power : snapshot?.power
  const lastReading = connection ? connection.lastReadingsAt
    : snapshot?.lastReadingsAt !== undefined ? snapshot.lastReadingsAt : snapshot?.lastSyncedAt
  const lastSync = formatSyncTime(lastReading).split(' ').at(-1)
  const connectedIcon = connection?.transport === 'ble' ? 'bluetooth' : connection?.transport ?? 'usb'
  const connectionIcon = phase === 'waiting' ? 'loading-four'
    : phase === 'failed' ? 'attention' : phase === 'disconnected' ? 'link-break' : connectedIcon
  const connectionLabel = { waiting: '等待连接', syncing: '等待同步', connected: '已连接', disconnected: '已断开', failed: '连接失败' }[phase]
  const battery = power?.source === 'battery' ? power : null
  const charging = battery && 'charging' in battery && battery.charging

  return (
    <div className="screen-status" role="group" aria-label="Status Bar">
      <PageGroup className="screen-page-group" onClick={onPageChange} title={onPageChange ? '下一页' : pageName} aria-label={onPageChange ? `${pageName}，第 ${index + 1} 页，共 ${pages?.length ?? 1} 页，点击下一页` : undefined}>
        <span className="screen-page-dots" role="img" aria-label={`第 ${index + 1} 页，共 ${pages?.length ?? 1} 页`}>{Array.from({ length: pages?.length ?? 1 }, (_, i) => <i key={i} data-active={i === index} />)}</span>
        <span className="screen-page">{pageName}</span>
      </PageGroup>
      <span className={`screen-info${surface === 'desktop' ? ' is-desktop' : ''}`}>
        <span title="最近应用读数的时间">Updated at {lastSync}</span>
        {surface === 'desktop' && <span title="应用版本">App v{version}</span>}
      </span>
      <span className="screen-status-right">
        {surface === 'desktop' ? <><Monitor size={14} role="img" aria-label="本机数据" /><span>本机</span></> : <>
        <span className={phase === 'waiting' ? 'screen-loading-icon' : undefined}>
          <DeviceIcon name={connectionIcon} size={14} label={connectionLabel} />
        </span>
        <span className="screen-power">
          {power?.source === 'external' ? <DeviceIcon name="plug-one" size={18} label="外部供电" />
            : battery ? <><DeviceIcon name={charging ? 'battery-charge' : 'battery-empty'} size={18}
                label={charging ? '正在充电' : '电池电量'} charge={battery.percent ?? undefined} />
                <span>{battery.percent == null ? '—' : `${battery.percent}%`}</span></>
              : <span aria-label="供电来源未知">—</span>}
        </span>
        </>}
      </span>
    </div>
  )
}

function ConnectionFailure({ reason }: { reason: string }) {
  const viewport = useRef<HTMLDivElement>(null)
  const text = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const box = viewport.current
    const line = text.current
    if (!box || !line) return
    let raf = 0
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)')
    const layout = () => {
      cancelAnimationFrame(raf)
      const distance = Math.max(0, line.scrollHeight - box.clientHeight)
      const travel = distance * 1000 / 20
      const duration = 1600 + 2 * travel
      const start = performance.now()
      box.scrollTop = 0
      box.dataset.scrolling = String(distance > 0 && !reduced.matches)
      if (!distance || reduced.matches) return
      const tick = (now: number) => {
        const elapsed = (now - start) % duration
        const moving = elapsed < 800 + travel ? elapsed - 800 : duration - elapsed
        const position = Math.max(0, Math.min(distance, moving * .02))
        box.scrollTop = position
        box.style.setProperty('--reason-top', position > 0 ? '4px' : '0px')
        box.style.setProperty('--reason-bottom', position < distance ? '4px' : '0px')
        raf = requestAnimationFrame(tick)
      }
      raf = requestAnimationFrame(tick)
    }
    const observer = new ResizeObserver(layout)
    observer.observe(line)
    observer.observe(box)
    reduced.addEventListener('change', layout)
    layout()
    return () => { cancelAnimationFrame(raf); observer.disconnect(); reduced.removeEventListener('change', layout) }
  }, [reason])
  return <div className="screen-message screen-failure">
    <span className="screen-failure-title"><DeviceIcon name="attention" size={16} label="错误" />连接失败</span>
    <div className="screen-failure-reason" ref={viewport} tabIndex={0}><div ref={text}>{reason}</div></div>
  </div>
}
