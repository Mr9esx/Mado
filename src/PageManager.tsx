import { Check, ChevronDown, Plus, Trash2 } from 'lucide-react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from './components/reui/color-picker-primitives'
import { useId, useRef, useState, type PointerEvent } from 'react'
import { display, type DisplayPage } from './layout'

type Props = { pages: DisplayPage[]; selected: string; disabled: boolean; rotationSeconds: number
  onSelect: (id: string) => void; onChange: (pages: DisplayPage[], selection?: string) => void
  onRotationChange: (seconds: number) => void }

type PageDrag = { id: string; pointerId: number; x: number; y: number; moved: boolean }

function PagePlayback({ seconds, disabled, onChange }: {
  seconds: number; disabled: boolean; onChange: (seconds: number) => void
}) {
  const description = useId()
  return <DropdownMenu>
    <DropdownMenuTrigger asChild>
      <Button className="page-playback-button" size="sm" variant="ghost" type="button" disabled={disabled}
        aria-describedby={description}>
        自动轮播<span className="page-playback-state">{seconds ? `每 ${seconds} 秒` : '关闭'}</span>
        <ChevronDown size={12} aria-hidden="true" />
      </Button>
    </DropdownMenuTrigger>
    <span id={description} className="sr-only">至少有两页时，应用布局后在桌面控件和设备上自动换页。</span>
    <DropdownMenuContent className="page-playback-menu" align="end" aria-label="自动轮播">
      <p className="page-playback-hint">桌面控件和设备会按页签顺序换页。至少有两页时，应用布局后生效。</p>
      <DropdownMenuRadioGroup value={String(seconds)} onValueChange={value => onChange(Number(value))}>
        {[0, 5, 10, 15, 30, 60].map(interval =>
          <DropdownMenuRadioItem key={interval} value={String(interval)}>
            {interval ? `每 ${interval} 秒换页` : '关闭自动轮播'}
            {interval === seconds && <Check className="ml-auto" size={13} aria-hidden="true" />}
          </DropdownMenuRadioItem>)}
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
}

export function PageManager({ pages, selected, disabled, rotationSeconds, onSelect, onChange, onRotationChange }: Props) {
  const [rename, setRename] = useState<{ id: string; text: string }>()
  const [preview, setPreview] = useState<{ from: number; to?: number }>()
  const drag = useRef<PageDrag | null>(null)
  const draggedClick = useRef<string | undefined>(undefined)
  const restoreFocus = useRef<string | undefined>(undefined)
  const name = rename?.text.trim()
  const validName = !!name && [...name].length <= 20

  function move(id: string, to: number) {
    const from = pages.findIndex(page => page.id === id)
    if (from < 0 || from === to || to < 0 || to >= pages.length) return
    const next = [...pages]
    const [page] = next.splice(from, 1)
    next.splice(to, 0, page)
    onChange(next, selected)
  }

  function startDrag(event: PointerEvent<HTMLButtonElement>, id: string) {
    if (event.button !== 0 || disabled || drag.current) return
    draggedClick.current = undefined
    if (pages.length < 2) return
    drag.current = { id, pointerId: event.pointerId, x: event.clientX, y: event.clientY, moved: false }
    event.currentTarget.setPointerCapture(event.pointerId)
  }

  function targetIndex(event: PointerEvent<HTMLDivElement>) {
    const tab = event.currentTarget.ownerDocument.elementFromPoint(event.clientX, event.clientY)
      ?.closest<HTMLElement>('[data-page-index]')
    return tab && event.currentTarget.contains(tab) ? Number(tab.dataset.pageIndex) : undefined
  }

  function moveDrag(event: PointerEvent<HTMLDivElement>) {
    const active = drag.current
    if (!active || active.pointerId !== event.pointerId || disabled) return
    if (!active.moved && Math.hypot(event.clientX - active.x, event.clientY - active.y) < 5) return
    active.moved = true
    const from = pages.findIndex(page => page.id === active.id)
    const to = targetIndex(event)
    setPreview(current => current?.from === from && current.to === to ? current : { from, to })
  }

  function finishDrag(event: PointerEvent<HTMLDivElement>, drop: boolean) {
    const active = drag.current
    if (!active || active.pointerId !== event.pointerId) return
    drag.current = null
    if (!active.moved) return
    draggedClick.current = active.id
    setPreview(undefined)
    const to = drop && !disabled ? targetIndex(event) : undefined
    if (to !== undefined) move(active.id, to)
  }

  function finishRename(entry: DisplayPage, save: boolean, focus: boolean) {
    if (focus) restoreFocus.current = entry.id
    if (save && !disabled && validName && entry.name !== name) {
      onChange(pages.map(page => page.id === entry.id ? { ...page, name } : page))
    }
    setRename(undefined)
  }
  function insert() {
    const page = { id: crypto.randomUUID(), name: `页面 ${pages.length + 1}`, layout: [] }
    setRename(undefined)
    onChange([...pages, page], page.id)
  }
  function remove(index: number) {
    const page = pages[index]
    const next = pages.filter(entry => entry.id !== page.id)
    setRename(undefined)
    onChange(next, page.id === selected ? next[Math.min(index, next.length - 1)].id : selected)
  }
  return <fieldset className="page-manager" disabled={disabled}>
    <legend className="sr-only">页面管理</legend>
    <div className="page-tabs-row">
      <div className="page-tabs" role="group" aria-label="编辑页面" data-dragging={!!preview}
        onPointerMove={moveDrag} onPointerUp={event => finishDrag(event, true)}
        onPointerCancel={event => finishDrag(event, false)} onLostPointerCapture={event => finishDrag(event, false)}>
        {pages.map((entry, index) => {
          const active = entry.id === selected
          const editing = active && rename?.id === entry.id
          const dropSide = preview?.to === index && preview.from !== index
            ? preview.from < index ? 'after' : 'before'
            : undefined
          return <div className="page-tab" data-active={active} data-page-index={index} data-editing={editing}
            data-drag-source={preview?.from === index} data-drop={dropSide} key={entry.id}>
            {editing ? <div className="page-tab-rename">
              <Input autoFocus variant="sm" aria-label="页面名称" aria-invalid={!validName} title="1–20 个字符"
                value={rename.text} onFocus={event => event.target.select()}
                onChange={event => setRename({ id: entry.id, text: event.target.value })}
                onBlur={() => finishRename(entry, true, false)}
                onKeyDown={event => {
                  if (event.nativeEvent.isComposing) return
                  if (event.key === 'Escape' || event.key === 'Enter') {
                    event.stopPropagation()
                    event.preventDefault()
                    if (event.key === 'Escape' || validName) finishRename(entry, event.key === 'Enter', true)
                  }
                }} />
            </div> : <Button className="page-tab-name" size="sm" variant="ghost" type="button" aria-pressed={active}
              title={[entry.name, active ? '点击改名' : '点击切换', pages.length > 1 && '拖动排序'].filter(Boolean).join(' · ')}
              aria-keyshortcuts={pages.length > 1 ? 'Alt+ArrowLeft Alt+ArrowRight' : undefined}
              ref={button => {
                if (!button || restoreFocus.current !== entry.id) return
                restoreFocus.current = undefined
                button.focus()
              }}
              onPointerDown={event => startDrag(event, entry.id)}
              onClick={event => {
                if (event.detail !== 0 && draggedClick.current === entry.id) return
                if (active) setRename({ id: entry.id, text: entry.name })
                else { setRename(undefined); onSelect(entry.id) }
              }}
              onKeyDown={event => {
                if (!event.altKey || (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight')) return
                event.preventDefault()
                move(entry.id, index + (event.key === 'ArrowLeft' ? -1 : 1))
              }}>
              <span>{entry.name || '未命名'}</span>
            </Button>}
            {pages.length > 1 && <Button className="page-tab-delete" size="xs" variant="ghost" type="button"
              mode="icon" aria-label={`删除页面「${entry.name}」`} title="删除页面" disabled={editing} onClick={() => remove(index)}>
              <Trash2 size={13} />
            </Button>}
          </div>
        })}
      </div>
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="新增页面" title={`新增页面（最多 ${display.maxPages} 页）`}
        disabled={pages.length >= display.maxPages} onClick={insert}><Plus size={14} /></Button>
      <PagePlayback seconds={rotationSeconds} disabled={disabled} onChange={onRotationChange} />
    </div>
  </fieldset>
}
