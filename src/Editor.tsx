import { Check, CircleAlert, Monitor, Plus, RefreshCw, Settings2 } from 'lucide-react'
import { useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent } from 'react'
import { Button } from '@/components/ui/button'
import { openDisplay, syncLayout, useDevice } from './device'
import { canPlace, cellAt, cellHeight, cellWidth, display, firstOpenCell, sameLayout, type LayoutItem } from './layout'
import { Screen } from './Screen'
import { WidgetReading } from './WidgetReading'
import { codexLabel, useCodexStatus } from './codex'
import { CodexSettings } from './CodexSettings'

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
  const { status: codexStatus, error: sourceError } = useCodexStatus()
  const [showSettings, setShowSettings] = useState(false)
  const [draft, setDraft] = useState<LayoutItem[] | null>(null)
  const [selectedId, select] = useState('')
  const [target, setTarget] = useState<{ item: LayoutItem; valid: boolean }>()
  const [draggingId, setDraggingId] = useState<string>()
  const [syncing, setSyncing] = useState(false)
  const [syncError, setSyncError] = useState('')
  const [displayError, setDisplayError] = useState('')
  const grid = useRef<HTMLDivElement>(null)
  const drag = useRef<Drag | null>(null)
  const layout = draft ?? snapshot?.layout ?? display.initialLayout
  const occupied = layout.reduce((count, item) => count + item.w * item.h, 0)
  const dirty = snapshot ? !sameLayout(layout, snapshot.layout) : false
  const SyncIcon = syncError ? CircleAlert : RefreshCw
  const DisplayIcon = displayError ? CircleAlert : Monitor

  function applyEdit(next: LayoutItem[], selection: string) {
    if (!sameLayout(layout, next)) setDraft(next)
    select(selection)
  }

  function removeCard(id: string) {
    applyEdit(layout.filter((item) => item.id !== id), selectedId === id ? '' : selectedId)
  }

  function place(item: LayoutItem) {
    if (!canPlace(layout, item)) return
    applyEdit(layout.some((entry) => entry.id === item.id)
      ? layout.map((entry) => entry.id === item.id ? item : entry)
      : [...layout, item], item.id)
  }

  function add(id: string) {
    if (layout.some((item) => item.id === id)) {
      select(id)
      return
    }
    const item = firstOpenCell(layout, id)
    if (item) place(item)
  }

  function beginDrag(event: PointerEvent<HTMLButtonElement>, item: LayoutItem, fromLibrary = false) {
    if (event.button !== 0 || drag.current) return
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
    setTarget(item && { item, valid: canPlace(layout, item) })
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
      receive(await syncLayout(layout, snapshot.revision))
    } catch (reason) {
      setSyncError(`同步失败。${String(reason)}`)
    } finally {
      setSyncing(false)
    }
  }

  async function showDisplay() {
    setDisplayError('')
    try {
      await openDisplay()
    } catch (reason) {
      setDisplayError(`无法打开虚拟屏。${String(reason)}`)
    }
  }

  return (
    <main className="studio" aria-label="布局编辑器" onPointerMove={moveDrag} onPointerUp={endDrag} onPointerCancel={clearDrag} onLostPointerCapture={clearDrag}
      onKeyDown={(event) => {
        if (!showSettings && event.key === 'Escape') {
          clearDrag()
          select('')
        }
      }}>
      <div className="window-grip" data-tauri-drag-region aria-hidden="true" />
      <div className="studio-canvas" style={editorGeometry}>
        <aside className="component-tools" aria-label="组件选择和操作">
          <div className="library-heading">
            <h2>组件</h2>
            <Button size="sm" variant="ghost" className="settings-button" onClick={() => setShowSettings(true)}>
              <Settings2 size={16} />设置
            </Button>
          </div>
          <div className="library-scroll">
            {display.groups.map((group) => (
              <section key={group.id} className="component-group" aria-label={group.name}>
                <div className="component-group-heading">
                  <h3>{group.name}</h3>
                  {group.id === 'codex' && (
                    <button type="button" className={`source-badge${codexStatus?.phase === 'connected' ? ' is-connected' : ''}`}
                      aria-label={`Codex 数据源设置，${codexLabel(codexStatus)}`}
                      onClick={() => setShowSettings(true)}>
                      {codexStatus?.phase === 'disabled' ? '连接' : <><i aria-hidden="true" />{codexLabel(codexStatus)}</>}
                    </button>
                  )}
                </div>
                <div className="component-list">
                  {display.widgets.filter((definition) => definition.group === group.id).map((definition) => {
                    const placed = layout.find((item) => item.id === definition.id)
                    const [defaultWidth, defaultHeight] = definition.sizes[0]
                    const item = placed ?? { id: definition.id, x: 0, y: 0, w: defaultWidth, h: defaultHeight }
                    const size = `${item.w} × ${item.h}`
                    return (
                      <button
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
                            <span className="screen-widget is-narrow library-thumbnail-card" style={thumbnailCard}>
                              <WidgetReading widget={definition} width={1} value={snapshot?.values[definition.id]} />
                            </span>
                          </span>
                        </span>
                        <span className="library-card-meta" aria-hidden="true">
                          <strong>{definition.name}</strong>
                          <span>{size}</span>
                        </span>
                        {placed ? <Check className="library-card-state" size={15} aria-hidden="true" /> : <Plus className="library-card-state" size={15} aria-hidden="true" />}
                      </button>
                    )
                  })}
                </div>
              </section>
            ))}
          </div>

        </aside>

        <section className="display-console" aria-label="屏幕预览">
          <div className="screen-meta">
            <h2>预览</h2>
            <span className="usage">{occupied} / {display.columns * display.rows} 格</span>
          </div>
          <div className="screen-stage">
            <Screen layout={layout} snapshot={snapshot} gridRef={grid}
              selected={selectedId} dragging={draggingId} target={target}
              onSelect={select} onRemove={removeCard} onCardPointerDown={beginDrag} onCardKeyDown={editWithKeys} />
          </div>

          <div className="display-toolbar">
            <Button size="sm" variant="ghost" className="action-button"
              title={displayError || undefined} onClick={showDisplay}>
              <DisplayIcon size={16} />虚拟屏
            </Button>
            <Button size="sm" className="action-button sync-button" disabled={!dirty || syncing}
              title={syncError || undefined} onClick={synchronize}>
              <SyncIcon size={14} className={syncing ? 'spinning' : ''} />{syncing ? '同步中' : '同步'}
            </Button>
          </div>
        </section>
      </div>
      {showSettings && <CodexSettings status={codexStatus} error={sourceError} onDismiss={() => setShowSettings(false)} />}
    </main>
  )
}
