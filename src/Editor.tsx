import { Check, ChevronDown, CircleAlert, Monitor, Plus, RefreshCw, Settings2, Terminal, Usb } from 'lucide-react'
import { useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react'
import { Button } from '@/components/ui/button'
import { openDesktopWidget, applyPages, useDevice } from './device'
import { canPlace, cardWidget, cellAt, cellHeight, cellWidth, defaultDisplayTheme, display, firstOpenCell, sameLayout, widgetCatalog, type DisplayPage, type DisplayTheme, type LayoutItem, type Widget } from './layout'
import { PageManager, PagePlayback } from './PageManager'
import { pageError, pagesFromSnapshot, samePages } from './pages'
import { Screen } from './Screen'
import { WidgetReading } from './WidgetReading'
import { codexLabel, useCodexStatus } from './codex'
import { AppSettings } from './AppSettings'
import { CardSettings } from './CardSettings'
import { useAppearance } from './appearance'
import { openCollectors } from './collectorApi'
import { useUsbDevice } from './usbDevice'
import { UsbSettings } from './UsbSettings'

type Drag = {
  pointer: number
  item: LayoutItem
  startX: number
  startY: number
  grabX: number
  grabY: number
  fromLibrary: boolean
  moved: boolean
}

const thumbnailHeight = 44
const thumbnailViewport = { width: thumbnailHeight * cellWidth / cellHeight, height: thumbnailHeight }
const thumbnailCard = { width: cellWidth, height: cellHeight, transform: `scale(${thumbnailHeight / cellHeight})` }
const editorGeometry = { '--preview-width': `${display.width}px` } as CSSProperties

const arrowOffsets: Record<string, [number, number]> = {
  ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0],
}

export function Editor() {
  const { snapshot, receive } = useDevice()
  const usb = useUsbDevice()
  const { appearance } = useAppearance()
  const { status: codexStatus, error: sourceError } = useCodexStatus()
  const [showSettings, setShowSettings] = useState<'appearance' | 'source' | null>(null)
  const [collapsedGroups, setCollapsedGroups] = useState(() => new Set<string>())
  const [draft, setDraft] = useState<{ pages: DisplayPage[]; rotationSeconds: number; theme: DisplayTheme; baseRevision: number } | null>(null)
  const [pageManagerKey, setPageManagerKey] = useState(0)
  const [showCardSettings, setShowCardSettings] = useState(false)
  const [showUsb, setShowUsb] = useState(false)
  const [pushInterval, setPushInterval] = useState(1000)
  const [selectedPageId, setSelectedPageId] = useState('')
  const [selectedId, select] = useState('')
  const [target, setTarget] = useState<{ item: LayoutItem; valid: boolean }>()
  const [draggingId, setDraggingId] = useState<string>()
  const [syncing, setSyncing] = useState(false)
  const [syncError, setSyncError] = useState('')
  const [displayError, setDisplayError] = useState('')
  const [collectorError, setCollectorError] = useState('')
  const grid = useRef<HTMLDivElement>(null)
  const drag = useRef<Drag | null>(null)
  const pages = draft?.pages ?? pagesFromSnapshot(snapshot)
  const currentPage = pages.find(page => page.id === selectedPageId) ?? pages[0]
  const rotationSeconds = draft?.rotationSeconds ?? snapshot?.rotationSeconds ?? 0
  const savedTheme = snapshot?.theme ?? { ...defaultDisplayTheme, active: appearance.displayTheme }
  const theme = draft?.theme ?? savedTheme
  const palette = theme[theme.active]
  const layout = currentPage.layout
  const catalog = widgetCatalog(snapshot)
  const selectedItem = layout.find(item => item.id === selectedId)
  const selectedWidget = selectedItem && catalog.get(selectedItem.id)
  const groups = new Map<string, Widget[]>()
  for (const widget of catalog.values()) {
    const group = groups.get(widget.group)
    if (group) group.push(widget)
    else groups.set(widget.group, [widget])
  }
  const occupied = layout.reduce((count, item) => count + item.w * item.h, 0)
  const resolved = [...new Set(pages.flatMap(page => page.layout.map(item => item.id)))].flatMap(id => catalog.get(id) ?? [])
  const dirty = snapshot ? snapshot.lastSyncedAt == null || !samePages(pages, pagesFromSnapshot(snapshot))
    || rotationSeconds !== (snapshot.rotationSeconds ?? 0)
    || JSON.stringify(theme) !== JSON.stringify(savedTheme)
    || (snapshot.appliedWidgets !== undefined && JSON.stringify(resolved) !== JSON.stringify(snapshot.appliedWidgets)) : false
  const validationError = pageError(pages)
  function editPages(next: DisplayPage[], selection?: string, rotation = rotationSeconds, nextTheme = theme) {
    if (syncing || !snapshot) return
    setDraft({ pages: next, rotationSeconds: rotation, theme: nextTheme, baseRevision: draft?.baseRevision ?? snapshot.revision })
    if (selection) { setSelectedPageId(selection); select('') }
    setSyncError('')
  }
  const SyncIcon = syncError ? CircleAlert : RefreshCw
  const DisplayIcon = displayError ? CircleAlert : Monitor

  function toggleGroup(id: string) {
    setCollapsedGroups(current => {
      const next = new Set(current)
      if (!next.delete(id)) next.add(id)
      return next
    })
  }

  function applyEdit(next: LayoutItem[], selection: string) {
    if (!sameLayout(layout, next)) editPages(pages.map(page => page.id === currentPage.id ? { ...page, layout: next } : page))
    select(selection)
  }

  function removeCard(id: string) {
    applyEdit(layout.filter((item) => item.id !== id), selectedId === id ? '' : selectedId)
  }

  function place(item: LayoutItem) {
    if (!canPlace(layout, item, catalog)) return
    applyEdit(layout.some((entry) => entry.id === item.id)
      ? layout.map((entry) => entry.id === item.id ? item : entry)
      : [...layout, item], item.id)
  }

  function add(id: string) {
    if (layout.some((item) => item.id === id)) {
      select(id)
      return
    }
    const item = firstOpenCell(layout, id, catalog)
    if (item) place(item)
  }

  function beginDrag(event: PointerEvent<HTMLButtonElement>, item: LayoutItem, fromLibrary = false) {
    if (syncing || event.button !== 0 || drag.current) return
    const cell = !fromLibrary && grid.current
      ? cellAt(grid.current.getBoundingClientRect(), event.clientX, event.clientY)
      : null
    drag.current = {
      pointer: event.pointerId,
      item,
      startX: event.clientX,
      startY: event.clientY,
      grabX: cell ? cell.x - item.x : 0,
      grabY: cell ? cell.y - item.y : 0,
      fromLibrary,
      moved: false,
    }
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  function proposedItem(event: PointerEvent, active: Drag): LayoutItem | undefined {
    if (!grid.current) return
    const cell = cellAt(grid.current.getBoundingClientRect(), event.clientX, event.clientY)
    if (cell) return { ...active.item, x: cell.x - active.grabX, y: cell.y - active.grabY }
  }

  function moveDrag(event: PointerEvent) {
    const active = drag.current
    if (!active || active.pointer !== event.pointerId) return
    if (!active.moved && Math.hypot(event.clientX - active.startX, event.clientY - active.startY) < 4) return
    active.moved = true
    setDraggingId(active.item.id)
    const item = proposedItem(event, active)
    setTarget(item && { item, valid: canPlace(layout, item, catalog) })
  }

  function clearDrag() {
    drag.current = null
    setTarget(undefined)
    setDraggingId(undefined)
  }

  function endDrag(event: PointerEvent) {
    const active = drag.current
    if (!active || active.pointer !== event.pointerId) return
    const item = proposedItem(event, active)
    clearDrag()
    if (active.moved) {
      if (item) place(item)
    } else if (active.fromLibrary) {
      add(active.item.id)
    } else {
      select(active.item.id)
    }
  }

  function editWithKeys(event: KeyboardEvent<HTMLButtonElement>, item: LayoutItem) {
    if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault()
      removeCard(item.id)
      return
    }
    const offset = arrowOffsets[event.key]
    if (!offset) return
    event.preventDefault()
    place({ ...item, x: item.x + offset[0], y: item.y + offset[1] })
  }

  async function synchronize() {
    if (!snapshot || syncing || !dirty) return
    setSyncing(true)
    setSyncError('')
    try {
      receive(await applyPages(pages, rotationSeconds, draft?.baseRevision ?? snapshot.revision, theme))
      clearDraft()
    } catch (reason) {
      setSyncError(`应用布局失败。${String(reason)}`)
    } finally {
      setSyncing(false)
    }
  }

  function clearDraft() {
    setDraft(null)
    setPageManagerKey(key => key + 1)
    setSyncError('')
  }

  async function showDisplay() {
    setDisplayError('')
    try {
      await openDesktopWidget()
    } catch (reason) {
      setDisplayError(`无法打开桌面控件。${String(reason)}`)
    }
  }

  return (
    <main className="studio studio-editor" aria-label="布局编辑器" onPointerMove={moveDrag} onPointerUp={endDrag} onPointerCancel={clearDrag} onLostPointerCapture={clearDrag}
      onKeyDown={(event) => {
        if (!showSettings && !showCardSettings && !showUsb && event.key === 'Escape') {
          clearDrag()
          select('')
        }
      }}>
      <header className="studio-titlebar">
        <div className="window-grip" data-tauri-drag-region aria-hidden="true" />
        <div className="studio-tools" role="group" aria-label="工具栏">
          <Button size="sm" variant="ghost" title={collectorError || undefined} onClick={() => {
            setCollectorError('')
            void openCollectors().catch(reason => setCollectorError(String(reason)))
          }}>
            {collectorError ? <CircleAlert size={15} /> : <Terminal size={15} />}采集器
          </Button>
          <Button size="sm" variant="ghost" onClick={() => {
            setShowUsb(true)
            void usb.refreshPorts().catch(() => {})
          }}>
            <Usb size={15} />{usb.connection?.phase === 'connected' ? 'USB 已连接' : 'USB 设备'}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setShowSettings('appearance')}>
            <Settings2 size={15} />设置
          </Button>
        </div>
      </header>
      <div className="studio-canvas" style={editorGeometry}>
        <aside className="component-tools" aria-label="组件库">
          <div className="library-heading">
            <h2>组件</h2>
          </div>
          <div className="library-scroll">
            {Array.from(groups, ([groupId, definitions]) => {
              const groupName = definitions[0].groupName || (groupId === 'codex' ? 'Codex' : groupId)
              const expanded = !collapsedGroups.has(groupId)
              const listId = `component-list-${groupId}`
              return (
                <section key={groupId} className="component-group" aria-label={groupName}>
                  <div className="component-group-heading">
                    <h3>
                      <Button size="sm" variant="ghost" type="button" className="component-group-toggle" aria-expanded={expanded}
                        aria-controls={listId} onClick={() => toggleGroup(groupId)}>
                        <ChevronDown size={14} aria-hidden="true" />
                        <span>{groupName}</span>
                      </Button>
                    </h3>
                    {groupId === 'codex' && (
                      <Button size="xs" variant="ghost" type="button" className={`source-badge${codexStatus?.phase === 'connected' ? ' is-connected' : ''}`}
                        aria-label={`Codex 数据源设置，${codexLabel(codexStatus)}`}
                        onClick={() => setShowSettings('source')}>
                        {codexStatus?.phase === 'disabled' ? '连接' : <><i aria-hidden="true" />{codexLabel(codexStatus)}</>}
                      </Button>
                    )}
                  </div>
                  <div className="component-list" id={listId} hidden={!expanded}>
                    {definitions.map((definition) => {
                      const placed = layout.find((item) => item.id === definition.id)
                      const [defaultWidth, defaultHeight] = definition.sizes[0]
                      const item = placed ?? { id: definition.id, x: 0, y: 0, w: defaultWidth, h: defaultHeight }
                      const size = `${item.h} × ${item.w}`
                      return (
                        <Button size="sm" variant="outline"
                          key={definition.id}
                          className={`library-card${selectedId === definition.id ? ' is-current' : ''}`}
                          type="button"
                          aria-label={`${placed ? '选择' : '添加'}${definition.name}，${size}`}
                          aria-pressed={selectedId === definition.id}
                          title={placed ? `拖动或选择${definition.name}` : `拖动或添加${definition.name}`}
                          onClick={(event) => { if (event.detail === 0) add(definition.id) }}
                          onPointerDown={(event) => beginDrag(event, item, true)}
                        >
                          <span className="library-thumbnail" aria-hidden="true">
                            <span className="library-thumbnail-viewport" style={thumbnailViewport}>
                              <span className="screen-widget is-narrow library-thumbnail-card" style={{
                                ...thumbnailCard, '--display-screen': palette.background, '--display-surface': palette.surface, '--display-ink': palette.text,
                              } as CSSProperties}>
                                <WidgetReading widget={cardWidget(definition, placed?.appearance)} width={1} value={snapshot?.values[definition.id]} />
                              </span>
                            </span>
                          </span>
                          <span className="library-card-meta" aria-hidden="true">
                            <strong>{definition.name}</strong>
                            <span>{size}</span>
                          </span>
                          {placed ? <Check className="library-card-state" size={15} aria-hidden="true" /> : <Plus className="library-card-state" size={15} aria-hidden="true" />}
                        </Button>
                      )
                    })}
                  </div>
                </section>
              )
            })}
          </div>
        </aside>

        <section className="display-console" aria-label="屏幕预览">
          <div className="screen-meta">
            <h2>预览</h2>
            <PagePlayback seconds={rotationSeconds} disabled={syncing || !snapshot}
              onChange={seconds => editPages(pages, undefined, seconds)} />
          </div>
          <PageManager key={pageManagerKey} pages={pages} selected={currentPage.id} disabled={syncing || !snapshot}
            onSelect={id => { setSelectedPageId(id); select(''); clearDrag() }} onChange={editPages} />
          <div className="preview-panel">
            <div className="screen-stage">
              <div>
                <Screen surface="desktop" pages={pages} pageId={currentPage.id} layout={layout} snapshot={snapshot} palette={theme} gridRef={grid}
                  selected={selectedId} dragging={draggingId} target={target}
                  onSelect={select} onRemove={removeCard} onCardPointerDown={beginDrag} onCardKeyDown={editWithKeys} />
                <span className="usage preview-usage">{occupied} / {display.columns * display.rows} 格</span>
              </div>
            </div>
            <div className="display-toolbar">
              <Button size="sm" variant="ghost" className="action-button"
                title={displayError || undefined} onClick={showDisplay}>
                <DisplayIcon size={16} />桌面控件
              </Button>
              {draft && <Button size="sm" variant="dim" type="button" disabled={syncing}
                title="恢复到上次应用的布局" onClick={clearDraft}>放弃更改</Button>}
              <Button size="sm" className="action-button sync-button" disabled={!snapshot || !dirty || syncing || !!validationError}
                title={syncError || '保存全部页面并同步到显示端'} onClick={synchronize}>
                <SyncIcon size={14} className={syncing ? 'spinning' : ''} />{syncing ? '应用中' : '应用布局'}
              </Button>
            </div>
          </div>
          {(syncError || validationError) && <p className="page-error" role="alert">{syncError || validationError}</p>}
          <Button size="sm" variant="outline" className="card-settings-button" disabled={!selectedWidget || syncing || !snapshot}
            title="选择卡片后设置颜色和样式"
            onClick={() => setShowCardSettings(true)}><Settings2 size={15} />组件设置{selectedWidget ? ` · ${selectedWidget.name}` : ''}</Button>
        </section>
      </div>
      {showSettings && <AppSettings initialTab={showSettings} status={codexStatus} error={sourceError} onDismiss={() => setShowSettings(null)}
        device={{ usb, interval: pushInterval, onIntervalChange: setPushInterval }}
        screenTheme={snapshot ? { value: theme, onChange: next => editPages(pages, undefined, rotationSeconds, next) } : undefined} />}
      {showCardSettings && selectedItem && selectedWidget && <CardSettings key={selectedWidget.id} item={selectedItem}
        widget={selectedWidget} theme={theme} fits={item => canPlace(layout, item, catalog)} onChange={place} onDismiss={() => setShowCardSettings(false)} />}
      {showUsb && <UsbSettings usb={usb} interval={pushInterval} onDismiss={() => setShowUsb(false)} />}
    </main>
  )
}
