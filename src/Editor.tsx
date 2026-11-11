import { ArrowLeft, Check, ChevronDown, CircleAlert, Monitor, Plus, RefreshCw, Terminal, Undo2 } from 'lucide-react'
import { Fragment, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react'
import { Button } from '@/components/ui/button'
import { NumberField, NumberFieldDecrement, NumberFieldGroup, NumberFieldIncrement, NumberFieldInput } from '@/components/reui/number-field'
import { openDesktopWidget, applyPages, useDevice } from './device'
import { canPlace, cardWidget, cellAt, cellHeight, cellWidth, defaultDisplayTheme, display, firstOpenCell, repositionCard, sameLayout, widgetCatalog, type DisplayPage, type DisplayTheme, type LayoutItem, type Widget } from './layout'
import { PageManager } from './PageManager'
import { pageError, pagesFromSnapshot } from './pages'
import { recordLayoutEdit, sameLayoutEdit, type LayoutHistory } from './layoutDraft'
import { Screen } from './Screen'
import { WidgetReading } from './WidgetReading'
import { codexLabel, useCodexStatus } from './codex'
import { AppThemeButton } from './AppThemeButton'
import { DeviceScreenControls } from './DeviceScreenControls'
import { CodexSettings } from './CodexSettings'
import { CardSettings } from './CardSettings'
import { ScreenAppearance } from './ScreenAppearance'
import { useCardPlayback } from './useCardPlayback'
import { useAppearance } from './appearance'
import { openCollectors } from './collectorApi'
import { displayTransportLabels, useDisplayConnection } from './displayConnection'
import { DisplayConnectionDialog } from './DisplayConnectionDialog'

type Drag = {
  pointer: number
  item: LayoutItem
  startX: number
  startY: number
  grabX: number
  grabY: number
  offsetX: number
  offsetY: number
  width: number
  height: number
  fromLibrary: boolean
  moved: boolean
}
type DragPreview = { item: LayoutItem; x: number; y: number; width: number; height: number }

const thumbnailHeight = 44
const thumbnailViewport = { width: thumbnailHeight * cellWidth / cellHeight, height: thumbnailHeight }
const thumbnailCard = { width: cellWidth, height: cellHeight, transform: `scale(${thumbnailHeight / cellHeight})` }
const editorGeometry = { '--preview-width': `${display.width}px` } as CSSProperties
const pushIntervalStep = 500

const arrowOffsets: Record<string, [number, number]> = {
  ArrowUp: [0, -1], ArrowDown: [0, 1], ArrowLeft: [-1, 0], ArrowRight: [1, 0],
}
const connectionLabels = {
  waiting: '未连接', syncing: '连接中', connected: '已连接', disconnected: '已断开', failed: '连接失败', reconnecting: '重连中',
}

export function Editor() {
  const { snapshot, receive } = useDevice()
  const device = useDisplayConnection()
  const { appearance } = useAppearance()
  const { status: codexStatus, error: sourceError } = useCodexStatus()
  const [showCodexSettings, setShowCodexSettings] = useState(false)
  const [collapsedGroups, setCollapsedGroups] = useState(() => new Set<string>())
  const [history, setHistory] = useState<LayoutHistory | null>(null)
  const [pageManagerKey, setPageManagerKey] = useState(0)
  const [showDevice, setShowDevice] = useState(false)
  const [pushInterval, setPushInterval] = useState(1000)
  const [selectedPageId, setSelectedPageId] = useState('')
  const [selectedId, select] = useState('')
  const [target, setTarget] = useState<{ item: LayoutItem; valid: boolean }>()
  const [dragPreview, setDragPreview] = useState<DragPreview>()
  const [syncing, setSyncing] = useState(false)
  const [syncError, setSyncError] = useState('')
  const [displayError, setDisplayError] = useState('')
  const [collectorError, setCollectorError] = useState('')
  const grid = useRef<HTMLDivElement>(null)
  const drag = useRef<Drag | null>(null)
  const saved = {
    pages: pagesFromSnapshot(snapshot),
    rotationSeconds: snapshot?.rotationSeconds ?? 0,
    theme: snapshot?.theme ?? { ...defaultDisplayTheme, active: appearance.displayTheme },
    baseRevision: snapshot?.revision ?? 0,
  }
  const draft = history?.draft
  const { pages, rotationSeconds, theme } = draft ?? saved
  const currentPage = pages.find(page => page.id === selectedPageId) ?? pages[0]
  const palette = theme[theme.active]
  const layout = currentPage.layout
  const catalog = widgetCatalog(snapshot)
  const draggedWidget = dragPreview && catalog.get(dragPreview.item.id)
  const selectedItem = layout.find(item => item.id === selectedId)
  const selectedWidget = selectedItem && catalog.get(selectedItem.id)
  const cardPlayback = useCardPlayback(selectedWidget ? `${currentPage.id}/${selectedWidget.id}` : undefined)
  const playback = cardPlayback.frame && selectedWidget ? { ...cardPlayback.frame, widgetId: selectedWidget.id } : undefined
  const groups = new Map<string, Widget[]>()
  for (const widget of catalog.values()) {
    const groupId = widget.moduleId ?? widget.group
    const group = groups.get(groupId)
    if (group) group.push(widget)
    else groups.set(groupId, [widget])
  }
  const resolved = [...new Set(pages.flatMap(page => page.layout.map(item => item.id)))].flatMap(id => catalog.get(id) ?? [])
  const dirty = snapshot ? snapshot.lastSyncedAt == null || !sameLayoutEdit(draft ?? saved, saved)
    || (snapshot.appliedWidgets !== undefined && JSON.stringify(resolved) !== JSON.stringify(snapshot.appliedWidgets)) : false
  const validationError = pageError(pages)
  function editPages(next: DisplayPage[], selection?: string, rotation = rotationSeconds, nextTheme = theme) {
    if (syncing || !snapshot) return
    setHistory(current => recordLayoutEdit(current, saved, { pages: next, rotationSeconds: rotation, theme: nextTheme }, currentPage.id))
    if (selection) {
      setSelectedPageId(selection)
      if (selection !== currentPage.id) select('')
    }
    setSyncError('')
  }
  const SyncIcon = syncError ? CircleAlert : RefreshCw
  const DisplayIcon = displayError ? CircleAlert : Monitor
  const { transport, phase = 'waiting', deviceId, port, reconnecting, reason } = device.status ?? {}
  const connectionPhase = reconnecting ? 'reconnecting' : phase
  const connectionLabel = `${transport ? `${displayTransportLabels[transport]} ` : ''}${connectionLabels[connectionPhase]}`
  const deviceName = deviceId && /^[0-9a-f]{12}$/i.test(deviceId) ? `Mado-${deviceId.slice(-4).toUpperCase()}`
    : port?.split('/').at(-1) || (transport ? 'Mado' : '设备')

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
    if (!snapshot || syncing || event.button !== 0 || drag.current) return
    const cell = !fromLibrary && grid.current
      ? cellAt(grid.current.getBoundingClientRect(), event.clientX, event.clientY)
      : null
    const rect = event.currentTarget.getBoundingClientRect()
    const width = fromLibrary ? item.w * cellWidth + (item.w - 1) * display.gap : rect.width
    const height = fromLibrary ? item.h * cellHeight + (item.h - 1) * display.gap : rect.height
    drag.current = {
      pointer: event.pointerId,
      item,
      startX: event.clientX,
      startY: event.clientY,
      grabX: cell ? cell.x - item.x : 0,
      grabY: cell ? cell.y - item.y : 0,
      offsetX: fromLibrary ? width / 2 : event.clientX - rect.left,
      offsetY: fromLibrary ? height / 2 : event.clientY - rect.top,
      width,
      height,
      fromLibrary,
      moved: false,
    }
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  function proposedItem(event: PointerEvent, active: Drag): LayoutItem | undefined {
    if (!grid.current) return
    const cell = cellAt(grid.current.getBoundingClientRect(), event.clientX, event.clientY)
    if (!cell) return
    if (layout.some(card => card.id === active.item.id)) {
      const hovered = layout.find(card => card.id !== active.item.id
        && cell.x >= card.x && cell.x < card.x + card.w && cell.y >= card.y && cell.y < card.y + card.h)
      if (hovered) return { ...active.item, x: hovered.x, y: hovered.y }
    }
    return { ...active.item, x: cell.x - active.grabX, y: cell.y - active.grabY }
  }

  function moveDrag(event: PointerEvent) {
    const active = drag.current
    if (!active || active.pointer !== event.pointerId) return
    if (!active.moved && Math.hypot(event.clientX - active.startX, event.clientY - active.startY) < 4) return
    active.moved = true
    setDragPreview({
      item: active.item, x: event.clientX - active.offsetX, y: event.clientY - active.offsetY,
      width: active.width, height: active.height,
    })
    const item = proposedItem(event, active)
    const destination = item && repositionCard(layout, item, catalog)?.find(card => card.id === item.id)
    setTarget(item && { item: destination ?? item, valid: !!destination })
  }

  function clearDrag() {
    drag.current = null
    setTarget(undefined)
    setDragPreview(undefined)
  }

  function endDrag(event: PointerEvent) {
    const active = drag.current
    if (!active || active.pointer !== event.pointerId) return
    const item = proposedItem(event, active)
    clearDrag()
    if (active.moved) {
      const next = item && repositionCard(layout, item, catalog)
      if (next && !syncing) applyEdit(next, item.id)
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
    const next = repositionCard(layout, { ...item, x: item.x + offset[0], y: item.y + offset[1] }, catalog)
    if (next) applyEdit(next, item.id)
  }

  async function synchronize() {
    if (!snapshot || syncing || !dirty) return
    cardPlayback.stop()
    setSyncing(true)
    setSyncError('')
    try {
      receive(await applyPages(pages, rotationSeconds, draft?.baseRevision ?? snapshot.revision, theme))
      restoreDraft(null)
    } catch (reason) {
      setSyncError(`应用布局失败。${String(reason)}`)
    } finally {
      setSyncing(false)
    }
  }

  function restoreDraft(next: LayoutHistory | null) {
    setHistory(next)
    setPageManagerKey(key => key + 1)
    setSyncError('')
  }

  function undoEdit() {
    if (!history || syncing) return
    cardPlayback.stop()
    const previousPages = history.previous?.draft.pages ?? saved.pages
    const previousPage = previousPages.find(page => page.id === history.pageBeforeEdit) ?? previousPages[0]
    setSelectedPageId(previousPage.id)
    if (previousPage.id !== currentPage.id || !previousPage.layout.some(item => item.id === selectedId)) select('')
    clearDrag()
    restoreDraft(history.previous)
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
        if (event.key === 'Escape' && !event.defaultPrevented && !event.nativeEvent.isComposing
          && !showCodexSettings && !showDevice && event.currentTarget.contains(event.target as Node)) {
          clearDrag()
          select('')
        }
      }}>
      <header className="studio-titlebar">
        <div className="window-grip" data-tauri-drag-region aria-hidden="true" />
        <div className="studio-tools" role="group" aria-label="工具栏">
          <Button size="sm" variant="ghost" type="button" title={displayError || '打开桌面控件'} onClick={showDisplay}>
            <DisplayIcon size={15} />桌面控件
          </Button>
          <AppThemeButton />
        </div>
      </header>
      <div className="studio-canvas" style={editorGeometry}>
        <aside className="component-tools" aria-label="组件库">
          <div className="library-heading">
            <h2>组件</h2>
            <Button size="xs" variant="ghost" type="button" title={collectorError || '打开采集器工作区'} onClick={() => {
              setCollectorError('')
              void openCollectors().catch(reason => setCollectorError(`无法打开采集器。${String(reason)}`))
            }}>
              {collectorError ? <CircleAlert size={14} aria-hidden="true" /> : <Terminal size={14} aria-hidden="true" />}管理采集器
            </Button>
          </div>
          <div className="library-scroll">
            {Array.from(groups, ([groupId, definitions]) => {
              const groupName = definitions[0].groupName || (groupId === 'codex' ? 'Codex' : groupId)
              const widgets = definitions[0].moduleId
                ? [...definitions].sort((a, b) => (a.instanceName ?? '').localeCompare(b.instanceName ?? '', 'zh-CN', { numeric: true }))
                : definitions
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
                        aria-label={`Codex 采集器，${codexLabel(codexStatus)}`}
                        onClick={() => setShowCodexSettings(true)}>
                        {codexStatus?.phase === 'disabled' ? '启用采集' : <><i aria-hidden="true" />{codexLabel(codexStatus)}</>}
                      </Button>
                    )}
                  </div>
                  <div className="component-list" id={listId} hidden={!expanded}>
                    {widgets.map((definition, index) => {
                      const placed = layout.find((item) => item.id === definition.id)
                      const [defaultWidth, defaultHeight] = definition.sizes[0]
                      const item = placed ?? { id: definition.id, x: 0, y: 0, w: defaultWidth, h: defaultHeight }
                      const size = `${item.h} × ${item.w}`
                      const instanceHeading = definition.instanceName && definition.name !== definition.instanceName
                        && widgets[index - 1]?.group !== definition.group
                      return (
                        <Fragment key={definition.id}>
                        {instanceHeading && <h4 className="px-1 text-xs text-muted-foreground">{definition.instanceName}</h4>}
                        <Button size="sm" variant="outline"
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
                        </Fragment>
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
          </div>
          <PageManager key={pageManagerKey} pages={pages} selected={currentPage.id} disabled={syncing || !snapshot}
            rotationSeconds={rotationSeconds} onRotationChange={seconds => editPages(pages, undefined, seconds)}
            onSelect={id => { setSelectedPageId(id); select(''); clearDrag() }} onChange={editPages} />
          <div className="preview-panel">
            <div className="screen-preview">
              <div className="screen-stage" onClick={event => {
                if (event.target instanceof Element && !event.target.closest('[data-widget]')) select('')
              }}>
                <Screen connection={device.screenConnection} pages={pages} pageId={currentPage.id} layout={layout} snapshot={snapshot} palette={theme} gridRef={grid}
                  selected={selectedId} dragging={dragPreview?.item.id} playback={playback} target={target}
                  onSelect={select} onRemove={removeCard} onCardPointerDown={beginDrag} onCardKeyDown={editWithKeys} />
              </div>
              <DeviceScreenControls device={device} />
            </div>
            <section className="preview-settings" aria-labelledby="preview-settings-title">
              <div className="preview-settings-header">
                <h3 id="preview-settings-title">{selectedWidget ? `组件设置 · ${selectedWidget.name}` : '屏幕配色'}</h3>
                {selectedWidget && <Button size="xs" variant="ghost" type="button" onClick={() => select('')}>
                  <ArrowLeft size={13} />返回屏幕配色
                </Button>}
              </div>
              <div className="preview-settings-body" key={selectedWidget ? `${currentPage.id}:${selectedWidget.id}` : 'screen'}>
                <fieldset className="preview-settings-fields" disabled={syncing || !snapshot}>
                  {selectedItem && selectedWidget
                    ? <CardSettings item={selectedItem} widget={selectedWidget} theme={theme}
                        currentValue={snapshot?.values[selectedItem.id]} fits={item => canPlace(layout, item, catalog)}
                        onChange={place} onPlay={cardPlayback.play} />
                    : <ScreenAppearance theme={theme} onChange={next => editPages(pages, undefined, rotationSeconds, next)} />}
                </fieldset>
              </div>
            </section>
            <div className="display-toolbar">
              <div className="display-device-controls" role="group" aria-label="设备控制">
                <Button size="xs" variant="outline" type="button" aria-haspopup="dialog"
                  aria-label={`设备连接，${deviceName} · ${connectionLabel}`}
                  title={reason || '打开设备连接'} onClick={() => {
                    setShowDevice(true)
                    void device.refreshPorts().catch(() => {})
                  }}>
                  <span className="device-connection-name">{deviceName}</span>
                  <span aria-hidden="true" className="text-muted-foreground">·</span>
                  <span role="status" data-phase={connectionPhase}
                    className="text-muted-foreground data-[phase=connected]:text-emerald-700 dark:data-[phase=connected]:text-emerald-400 data-[phase=failed]:text-destructive">
                    {connectionLabel}
                  </span>
                </Button>
                <NumberField id="device-push-interval" min={500} max={60000} step={pushIntervalStep}
                  smallStep={pushIntervalStep} largeStep={pushIntervalStep}
                  required allowOutOfRange format={{ useGrouping: false }} value={pushInterval}
                  onValueChange={value => {
                    if (value !== null && Number.isInteger(value) && value >= 500 && value <= 60000) setPushInterval(value)
                  }}>
                  <NumberFieldGroup className="gap-1 overflow-hidden ps-1.5 text-xs" title="500–60000 毫秒，下次连接生效。蓝牙至少间隔 2000 毫秒。">
                    <label htmlFor="device-push-interval" className="shrink-0 whitespace-nowrap text-muted-foreground">
                      推送间隔<span className="sr-only">（毫秒）</span>
                    </label>
                    <NumberFieldInput className="w-10 shrink-0 text-right" />
                    <span className="text-muted-foreground" aria-hidden="true">ms</span>
                    <div className="grid h-full w-[18px] shrink-0 grid-rows-2 divide-y divide-input border-s border-input">
                      <NumberFieldIncrement aria-label="增加推送间隔" />
                      <NumberFieldDecrement aria-label="减少推送间隔" />
                    </div>
                  </NumberFieldGroup>
                </NumberField>
              </div>
              <div className="layout-actions">
                {history && <Button size="xs" variant="outline" className="action-button" type="button" disabled={syncing}
                  title="撤销当前布局草稿的最近一步" onClick={undoEdit}><Undo2 size={14} />撤销改动</Button>}
                <Button size="xs" className="action-button sync-button" disabled={!snapshot || !dirty || syncing || !!validationError}
                  title={syncError || '保存全部页面并同步到显示端'} onClick={synchronize}>
                  <SyncIcon size={14} className={syncing ? 'spinning' : ''} />{syncing ? '应用中' : '应用布局'}
                </Button>
              </div>
            </div>
          </div>
          {(syncError || validationError) && <p className="page-error" role="alert">{syncError || validationError}</p>}
        </section>
      </div>
      {dragPreview && draggedWidget && <div className={`screen-widget card-drag-preview${dragPreview.item.w === 1 ? ' is-narrow' : ''}`} aria-hidden="true" style={{
        left: dragPreview.x, top: dragPreview.y, width: dragPreview.width, height: dragPreview.height,
        '--display-screen': palette.background, '--display-surface': palette.surface, '--display-ink': palette.text,
      } as CSSProperties}>
        <WidgetReading widget={cardWidget(draggedWidget, dragPreview.item.appearance)}
          width={dragPreview.item.w} height={dragPreview.item.h} value={snapshot?.values[dragPreview.item.id]} />
      </div>}
      {showCodexSettings && <CodexSettings status={codexStatus} error={sourceError} onDismiss={() => setShowCodexSettings(false)} />}
      {showDevice && <DisplayConnectionDialog device={device} interval={pushInterval} onDismiss={() => setShowDevice(false)} />}
    </main>
  )
}
