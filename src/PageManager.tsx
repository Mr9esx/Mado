import { Pencil, Plus, Trash2, Undo2 } from 'lucide-react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { useState } from 'react'
import { display, type DisplayPage } from './layout'

type Props = { pages: DisplayPage[]; selected: string; disabled: boolean
  onSelect: (id: string) => void; onChange: (pages: DisplayPage[], selection?: string) => void }

const pageDragType = 'application/x-mado-page'

export function PagePlayback({ seconds, disabled, onChange }: {
  seconds: number; disabled: boolean; onChange: (seconds: number) => void
}) {
  return <Select value={String(seconds)} disabled={disabled} onValueChange={value => onChange(Number(value))}>
    <SelectTrigger size="sm" className="page-playback-select" aria-label="展示方式" title="展示方式"><SelectValue /></SelectTrigger>
    <SelectContent><SelectItem value="0">手动切换</SelectItem>
      {[5, 10, 15, 30, 60].map(value => <SelectItem key={value} value={String(value)}>每 {value} 秒轮播</SelectItem>)}
    </SelectContent>
  </Select>
}

export function PageManager({ pages, selected, disabled, onSelect, onChange }: Props) {
  const [removed, setRemoved] = useState<{ page: DisplayPage; index: number }>()
  const [rename, setRename] = useState<{ id: string; text: string }>()
  const name = rename?.text.trim()
  const validName = !!name && [...name].length <= 20
  function move(id: string, to: number) {
    const from = pages.findIndex(page => page.id === id)
    if (from < 0 || from === to || to < 0 || to >= pages.length) return
    const next = [...pages]
    const [page] = next.splice(from, 1)
    next.splice(to, 0, page)
    onChange(next)
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
    setRemoved({ page, index })
    onChange(next, next[Math.min(index, next.length - 1)].id)
  }
  return <fieldset className="page-manager" disabled={disabled}>
    <legend className="sr-only">页面管理</legend>
    <div className="page-tabs-row">
      <div className="page-tabs" role="group" aria-label="编辑页面">
        {pages.map((entry, index) => {
          const active = entry.id === selected
          if (active && rename?.id === entry.id) return <form className="page-tab-rename" key={entry.id}
            onSubmit={event => {
              event.preventDefault()
              if (!validName) return
              if (entry.name !== name) onChange(pages.map(page => page.id === entry.id ? { ...page, name } : page))
              setRename(undefined)
            }} onKeyDown={event => {
              if (event.key === 'Escape') { event.stopPropagation(); setRename(undefined) }
            }}>
            <Input autoFocus variant="sm" aria-label="页面名称" aria-invalid={!validName} title="1–20 个字符" value={rename.text}
              onFocus={event => event.target.select()}
              onChange={event => setRename({ id: entry.id, text: event.target.value })} />
            <Button size="sm" variant="secondary" type="submit" disabled={!validName}>保存</Button>
            <Button size="sm" variant="ghost" type="button" onClick={() => setRename(undefined)}>取消</Button>
          </form>
          return <div className="page-tab" data-active={active} key={entry.id} draggable={!disabled && pages.length > 1}
            onDragStart={event => {
              event.dataTransfer.setData(pageDragType, entry.id)
              event.dataTransfer.effectAllowed = 'move'
            }}
            onDragOver={event => {
              if (!disabled && event.dataTransfer.types.includes(pageDragType)) {
                event.preventDefault()
                event.dataTransfer.dropEffect = 'move'
              }
            }}
            onDrop={event => {
              if (disabled || !event.dataTransfer.types.includes(pageDragType)) return
              event.preventDefault()
              move(event.dataTransfer.getData(pageDragType), index)
            }}>
            <Button className="page-tab-name" size="sm" variant="ghost" type="button" aria-pressed={active}
              title={`${entry.name} · ${active ? '点击改名，' : ''}拖动排序`}
              onClick={() => {
                if (active) setRename({ id: entry.id, text: entry.name })
                else { setRename(undefined); onSelect(entry.id) }
              }}
              onKeyDown={event => {
                if (!event.altKey || (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight')) return
                event.preventDefault()
                move(entry.id, index + (event.key === 'ArrowLeft' ? -1 : 1))
              }}>
              {index + 1}<span>{entry.name || '未命名'}</span>{active && <Pencil aria-hidden="true" />}
            </Button>
            {active && pages.length > 1 && <Button className="page-tab-delete" size="xs" variant="ghost" type="button"
              mode="icon" aria-label={`删除页面「${entry.name}」`} title="删除当前页" onClick={() => remove(index)}>
              <Trash2 size={13} />
            </Button>}
          </div>
        })}
      </div>
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="新增页面" title={`新增页面（最多 ${display.maxPages} 页）`}
        disabled={pages.length >= display.maxPages} onClick={insert}><Plus size={14} /></Button>
    </div>
    {removed && !pages.some(entry => entry.id === removed.page.id) && <Button size="xs" variant="ghost" type="button" className="page-undo"
      disabled={pages.length >= display.maxPages} onClick={() => {
        const next = [...pages]; next.splice(removed.index, 0, removed.page); onChange(next, removed.page.id)
        setRemoved(undefined)
        setRename(undefined)
      }}><Undo2 size={12} />撤销删除「{removed.page.name}」</Button>}
  </fieldset>
}
