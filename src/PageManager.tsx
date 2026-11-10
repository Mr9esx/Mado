import { ArrowLeft, ArrowRight, Copy, Plus, Trash2, Undo2 } from 'lucide-react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { useState } from 'react'
import { display, type DisplayPage } from './layout'

type Props = { pages: DisplayPage[]; selected: string; rotationSeconds: number; disabled: boolean
  onSelect: (id: string) => void; onChange: (pages: DisplayPage[], selection?: string) => void
  onRotationChange: (seconds: number) => void }
export function PageManager({ pages, selected, rotationSeconds, disabled, onSelect, onChange, onRotationChange }: Props) {
  const [removed, setRemoved] = useState<{ page: DisplayPage; index: number }>()
  const index = Math.max(0, pages.findIndex(page => page.id === selected))
  const page = pages[index]
  function insert(copy: boolean) {
    const next = { id: crypto.randomUUID(), name: copy ? `${page.name.slice(0, 17)} 副本` : `页面 ${pages.length + 1}`, layout: copy ? page.layout.map(item => ({ ...item })) : [] }
    onChange([...pages.slice(0, index + 1), next, ...pages.slice(index + 1)], next.id)
  }
  function move(offset: number) {
    const next = [...pages]
    ;[next[index], next[index + offset]] = [next[index + offset], next[index]]
    onChange(next)
  }
  return <fieldset className="page-manager" disabled={disabled}>
    <legend className="sr-only">页面管理</legend>
    <div className="page-tabs" role="group" aria-label="编辑页面">
      {pages.map((entry, i) => <Button size="sm" variant="ghost" type="button" key={entry.id} selected={entry.id === page.id} aria-pressed={entry.id === page.id}
        onClick={() => onSelect(entry.id)} title={entry.name}>{i + 1}<span>{entry.name || '未命名'}</span></Button>)}
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="新增页面" title={`新增页面（最多 ${display.maxPages} 页）`}
        disabled={pages.length >= display.maxPages} onClick={() => insert(false)}><Plus size={14} /></Button>
    </div>
    <div className="page-settings">
      <Input variant="sm" aria-label="页面名称" value={page.name} maxLength={20}
        onChange={event => onChange(pages.map(entry => entry.id === page.id ? { ...entry, name: event.target.value } : entry))} />
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="页面前移" title="前移" disabled={index === 0} onClick={() => move(-1)}><ArrowLeft size={14} /></Button>
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="页面后移" title="后移" disabled={index === pages.length - 1} onClick={() => move(1)}><ArrowRight size={14} /></Button>
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="复制页面" title="复制页面" disabled={pages.length >= display.maxPages} onClick={() => insert(true)}><Copy size={14} /></Button>
      <Button size="sm" variant="ghost" type="button" mode="icon" aria-label="删除页面" title="删除页面" disabled={pages.length === 1} onClick={() => {
        setRemoved({ page, index }); const next = pages.filter(entry => entry.id !== page.id)
        onChange(next, next[Math.min(index, next.length - 1)].id)
      }}><Trash2 size={14} /></Button>
    </div>
    <div className="page-playback">
      <label htmlFor="page-playback">展示方式</label>
      <Select value={String(rotationSeconds)} disabled={disabled} onValueChange={value => onRotationChange(Number(value))}>
        <SelectTrigger id="page-playback" size="sm" className="page-playback-select" aria-label="展示方式"><SelectValue /></SelectTrigger>
        <SelectContent><SelectItem value="0">手动切换</SelectItem>
          {[5, 10, 15, 30, 60].map(seconds => <SelectItem key={seconds} value={String(seconds)}>每 {seconds} 秒轮播</SelectItem>)}
        </SelectContent>
      </Select>
      <span>{pages.length} / {display.maxPages} 页</span>
    </div>
    {removed && !pages.some(entry => entry.id === removed.page.id) && <Button size="sm" variant="ghost" type="button" className="page-undo"
      disabled={pages.length >= display.maxPages} onClick={() => {
        const next = [...pages]; next.splice(removed.index, 0, removed.page); onChange(next, removed.page.id); setRemoved(undefined)
      }}><Undo2 size={12} />撤销删除「{removed.page.name}」</Button>}
  </fieldset>
}
