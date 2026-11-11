import { useState } from 'react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { cardWidget, type CardAppearance, type DisplayTheme, type LayoutItem, type NumberAnimation, type Reading, type StateStyle, type Widget, type WidgetValue } from './layout'
import { DeviceIcon, type DeviceIconName } from './DeviceIcon'
import { ColorField } from './ColorField'
import type { CardTrial } from './useCardPlayback'

const stateIcons: Record<string, DeviceIconName> = {
  running: 'play', completed: 'check-one', interrupted: 'forbid', error: 'attention', unknown: 'help',
}

export function CardSettings({ item, widget, theme, currentValue, fits, onChange, onPlay }: {
  item: LayoutItem; widget: Widget; theme: DisplayTheme; fits: (item: LayoutItem) => boolean
  currentValue?: WidgetValue | null; onChange: (item: LayoutItem) => void; onPlay: (trial: CardTrial) => void
}) {
  const states = Object.entries(widget.stateLabels ?? {})
  const [state, setState] = useState(states[0]?.[0] ?? '')
  const [from, setFrom] = useState('80')
  const [to, setTo] = useState('90')
  const validTrial = !!from && !!to && Number.isFinite(Number(from)) && Number.isFinite(Number(to))
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
    if (!validTrial) return
    const current = currentValue && typeof currentValue === 'object' && 'kind' in currentValue ? currentValue : undefined
    const reading = (value: number): Reading => ({
      ...current, kind: widget.kind, value, ...(widget.kind === 'progress' && { ratio: value / 100 }),
    })
    onPlay({ from: reading(Number(from)), to: reading(Number(to)), holdMs: 1150 })
  }
  function playAlert() {
    const reading: Reading = { kind: 'state', state, detail: '预览状态提醒' }
    onPlay({ from: reading, to: { ...reading, notify: true }, holdMs: (stateStyle.alertDurationMs ?? 2000) + 200 })
  }
  return <div className="display-card-settings">
    <div className="display-card-main">
      <fieldset>
        <legend>规格（行 × 列）</legend>
        <div className="display-card-options">
          {widget.sizes.map(([w, h]) => <Button key={`${w}x${h}`} size="xs" variant="outline"
            aria-pressed={item.w === w && item.h === h} disabled={!fits({ ...item, w, h })}
            onClick={() => onChange({ ...item, w, h })}>{h} × {w}</Button>)}
        </div>
      </fieldset>
      <ColorField label={colorLabel} value={resolved.accentColor ?? palette.text} onChange={accentColor => editAppearance({ accentColor })} />
      {appearance.accentColor && <Button size="xs" variant="ghost" onClick={() => editAppearance({ accentColor: undefined })}>恢复默认颜色</Button>}
    </div>
    {numeric && <div className="display-number-animation" role="group" aria-label="数值动画和试播">
      <div className="display-animation-mode">
        <span aria-hidden="true">数值动画</span>
        <Select value={resolved.numberAnimation ?? 'none'} onValueChange={value => editAppearance({ numberAnimation: value as NumberAnimation })}>
          <SelectTrigger aria-label="数值动画" size="xs"><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem value="none">不播放</SelectItem><SelectItem value="digitRoll">逐位滚动</SelectItem><SelectItem value="count">计数</SelectItem></SelectContent>
        </Select>
      </div>
      <label>起始值 <Input variant="sm" type="number" step="any" value={from} onChange={event => setFrom(event.target.value)} /></label>
      <label>结束值 <Input variant="sm" type="number" step="any" value={to} onChange={event => setTo(event.target.value)} /></label>
      <Button variant="outline" size="xs" type="button" aria-label="试播数值动画" title="在预览卡片中试播"
        disabled={!validTrial} onClick={playNumber}>试播</Button>
    </div>}
    {supportsAlert && <fieldset>
      <legend>状态颜色和提醒</legend>
      <Select value={state} onValueChange={setState}>
        <SelectTrigger size="sm" aria-label="示例状态"><SelectValue /></SelectTrigger>
        <SelectContent>{states.map(([key, name]) => <SelectItem key={key} value={key}>
          <span className="display-state-option"><span aria-hidden="true"><DeviceIcon name={stateIcons[key] ?? 'help'} size={16} label="" /></span>{name}</span>
        </SelectItem>)}</SelectContent>
      </Select>
      <ColorField label="状态颜色" value={stateStyle.color ?? resolved.accentColor ?? palette.text} onChange={color => editState({ color })} />
      <div className="display-card-options">
        <label>闪烁时长（秒） <Input variant="sm" type="number" min={0} max={10} step={1}
          value={(stateStyle.alertDurationMs ?? 2000) / 1000}
          onChange={event => {
            const seconds = event.target.valueAsNumber
            if (Number.isInteger(seconds) && seconds >= 0 && seconds <= 10) editState({ alertDurationMs: seconds * 1000 })
          }} /></label>
        <Button variant="outline" size="sm" onClick={playAlert}>播放提醒</Button>
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
  </div>
}
