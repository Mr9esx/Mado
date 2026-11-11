import { useEffect, useRef, useState, type CSSProperties } from 'react'
import { Button } from './components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { Input } from './components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { cardWidget, type CardAppearance, type DisplayTheme, type LayoutItem, type NumberAnimation, type StateStyle, type Widget, type WidgetValue } from './layout'
import { WidgetReading } from './WidgetReading'
import { DeviceIcon, type DeviceIconName } from './DeviceIcon'

const presets = [
  ['默认', '#F1F1F1'], ['紫', '#9267CD'], ['红', '#D63F41'], ['黄', '#D3AD3E'], ['绿', '#48B34A'],
  ['青绿', '#349E9A'], ['蓝', '#397ADC'], ['玫粉', '#C96091'], ['橙', '#DE7B3C'], ['蓝灰', '#9BA5B3'],
]
const stateIcons: Record<string, DeviceIconName> = {
  running: 'play', completed: 'check-one', interrupted: 'forbid', error: 'attention', unknown: 'help',
}

export function ColorField({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  const [raw, setRaw] = useState(value)
  useEffect(() => setRaw(value), [value])
  const valid = /^#[0-9a-f]{6}$/i.test(raw)
  return <div className="display-color-field">
    <label>
      <span>{label}</span>
      <Input variant="sm" value={raw} maxLength={7} aria-invalid={!valid} spellCheck={false}
        onBlur={() => setRaw(value)}
        onChange={event => {
          const next = event.target.value
          setRaw(next)
          if (/^#[0-9a-f]{6}$/i.test(next)) onChange(next.toUpperCase())
        }} />
    </label>
    <input type="color" value={value} aria-label={`选择${label}`}
      onChange={event => onChange(event.target.value.toUpperCase())} />
  </div>
}

export function CardSettings({ item, widget, theme, fits, onChange, onDismiss }: {
  item: LayoutItem; widget: Widget; theme: DisplayTheme; fits: (item: LayoutItem) => boolean
  onChange: (item: LayoutItem) => void; onDismiss: () => void
}) {
  const states = Object.entries(widget.stateLabels ?? {})
  const [state, setState] = useState(states[0]?.[0] ?? '')
  const [sequence, setSequence] = useState(0)
  const [from, setFrom] = useState(80)
  const [to, setTo] = useState(90)
  const [example, setExample] = useState<WidgetValue>()
  const animationFrame = useRef(0)
  useEffect(() => () => cancelAnimationFrame(animationFrame.current), [])
  const appearance = item.appearance ?? {}
  const resolved = cardWidget(widget, appearance)
  const stateStyle = resolved.stateStyles?.[state] ?? {}
  const numeric = ['metric', 'progress', 'trend', 'range'].includes(widget.kind) && widget.format !== 'countdown'
  const isState = widget.kind === 'state' || widget.kind === 'task'
  const supportsAlert = isState && widget.supportsNotify && states.length > 0
  const palette = theme[theme.active]
  const colorLabel = widget.kind === 'chart' ? '图表颜色' : widget.kind === 'progress' ? '进度颜色' : '标记颜色'
  function editAppearance(patch: Partial<CardAppearance>) {
    onChange({ ...item, appearance: { ...appearance, ...patch } })
  }
  function editState(patch: Partial<StateStyle>) {
    const stateStyles = { ...appearance.stateStyles, [state]: { ...appearance.stateStyles?.[state], ...patch } }
    editAppearance({ stateStyles })
  }
  function playNumber() {
    const reading = (value: number) => ({ kind: widget.kind, value, ...(widget.kind === 'progress' && { ratio: value / 100 }) })
    setExample(reading(from))
    setSequence(value => value + 1)
    cancelAnimationFrame(animationFrame.current)
    animationFrame.current = requestAnimationFrame(() => {
      animationFrame.current = requestAnimationFrame(() => setExample(reading(to)))
    })
  }
  const sample = supportsAlert
    ? { kind: 'state', state, detail: '预览状态提醒', notify: sequence > 0 }
    : example ?? (numeric ? { kind: widget.kind, value: from, ratio: from / 100 } : undefined)
  return <Dialog open onOpenChange={open => { if (!open) onDismiss() }}>
    <DialogContent className="settings-dialog">
      <DialogHeader>
        <DialogTitle>{widget.name} · 组件设置</DialogTitle>
        <DialogDescription>设置保存在草稿中，应用布局后同步到显示端。</DialogDescription>
      </DialogHeader>
      <div className="display-card-settings">
        <fieldset>
          <legend>规格（行 × 列）</legend>
          <div className="display-card-options">
            {widget.sizes.map(([w, h]) => <Button key={`${w}x${h}`} size="sm" variant="outline"
              aria-pressed={item.w === w && item.h === h} disabled={!fits({ ...item, w, h })}
              onClick={() => onChange({ ...item, w, h })}>{h} × {w}</Button>)}
          </div>
        </fieldset>
        <ColorField label={colorLabel} value={resolved.accentColor ?? palette.text} onChange={accentColor => editAppearance({ accentColor })} />
        {appearance.accentColor && <Button size="xs" variant="ghost" onClick={() => editAppearance({ accentColor: undefined })}>恢复默认颜色</Button>}
        {numeric && <fieldset>
          <legend>数值动画</legend>
          <Select value={resolved.numberAnimation ?? 'none'} onValueChange={value => editAppearance({ numberAnimation: value as NumberAnimation })}>
            <SelectTrigger aria-label="数值动画" size="sm"><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem value="none">不播放</SelectItem><SelectItem value="digitRoll">逐位滚动</SelectItem><SelectItem value="count">计数</SelectItem></SelectContent>
          </Select>
          <div className="display-card-options">
            <label>起始值 <Input variant="sm" type="number" value={from} onChange={event => setFrom(Number(event.target.value))} /></label>
            <label>结束值 <Input variant="sm" type="number" value={to} onChange={event => setTo(Number(event.target.value))} /></label>
            <Button variant="outline" size="sm" onClick={playNumber}>试播数值动画</Button>
          </div>
        </fieldset>}
        {supportsAlert && <fieldset>
          <legend>状态颜色和提醒</legend>
          <Select value={state} onValueChange={setState}>
            <SelectTrigger size="sm" aria-label="示例状态"><SelectValue /></SelectTrigger>
            <SelectContent>{states.map(([key, name]) => <SelectItem key={key} value={key}>
              <span className="display-state-option"><span aria-hidden="true"><DeviceIcon name={stateIcons[key] ?? 'help'} size={16} label="" /></span>{name}</span>
            </SelectItem>)}</SelectContent>
          </Select>
          <ColorField label="状态颜色" value={stateStyle.color ?? resolved.accentColor ?? palette.text} onChange={color => editState({ color })} />
          <div className="display-color-presets" role="group" aria-label="预设颜色">
            {presets.map(([name, color]) => <button key={color} type="button" title={name} aria-label={name}
              aria-pressed={stateStyle.color?.toUpperCase() === color} style={{ backgroundColor: color }}
              onClick={() => editState({ color })} />)}
          </div>
          <div className="display-card-options">
            <label>闪烁时长（秒） <Input variant="sm" type="number" min={0} max={10} step={1}
              value={(stateStyle.alertDurationMs ?? 2000) / 1000}
              onChange={event => {
                const seconds = event.target.valueAsNumber
                if (Number.isInteger(seconds) && seconds >= 0 && seconds <= 10) editState({ alertDurationMs: seconds * 1000 })
              }} /></label>
            <Button variant="outline" size="sm" onClick={() => setSequence(value => value + 1)}>播放提醒</Button>
          </div>
          <label className="display-check">
            <input type="checkbox" checked={!stateStyle.alertText || stateStyle.alertText === 'auto'}
              onChange={event => editState({ alertText: event.target.checked ? 'auto' : '#000000' })} />自动选择提醒字色
          </label>
          {stateStyle.alertText && stateStyle.alertText !== 'auto' &&
            <ColorField label="提醒字色" value={stateStyle.alertText} onChange={alertText => editState({ alertText })} />}
          {appearance.stateStyles?.[state] && <Button variant="ghost" size="xs" onClick={() => {
            const stateStyles = { ...appearance.stateStyles }
            delete stateStyles[state]
            editAppearance({ stateStyles })
          }}>恢复此状态的默认样式</Button>}
        </fieldset>}
        {(numeric || supportsAlert) && <div className="display-card-sample" aria-label="组件试播预览" style={{
          '--display-screen': palette.background, '--display-surface': palette.surface, '--display-ink': palette.text,
        } as CSSProperties}><WidgetReading key={numeric ? sequence : 'state'} widget={resolved} width={1} value={sample} sequence={sequence} /></div>}
      </div>
    </DialogContent>
  </Dialog>
}
