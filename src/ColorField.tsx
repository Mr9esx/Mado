import { useId, useState } from 'react'
import {
  ColorPicker, ColorPickerArea, ColorPickerContent, ColorPickerInput,
  ColorPickerPanel, ColorPickerSlider, ColorPickerSwatches,
} from './components/reui/color-picker'

const presets = [
  { label: '默认', value: '#F1F1F1' }, { label: '紫', value: '#9267CD' },
  { label: '红', value: '#D63F41' }, { label: '黄', value: '#D3AD3E' },
  { label: '绿', value: '#48B34A' }, { label: '青绿', value: '#349E9A' },
  { label: '蓝', value: '#397ADC' }, { label: '玫粉', value: '#C96091' },
  { label: '橙', value: '#DE7B3C' }, { label: '蓝灰', value: '#9BA5B3' },
]

export function ColorField({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  const id = useId()
  const [pending, setPending] = useState<string | null>(null)
  return <div className="display-color-field">
    <label htmlFor={id} id={`${id}-label`}>{label}</label>
    <ColorPicker id={id} aria-labelledby={`${id}-label`} value={pending ?? value} format="hex"
      i18n={{ labels: { openPicker: `选择${label}`, area: '饱和度和亮度', hue: '色相', swatches: '预设颜色', copied: '已复制' } }}
      onOpenChange={open => { if (!open) setPending(null) }}
      onValueChange={next => setPending(next?.toUpperCase() === value.toUpperCase() ? null : next)}
      onValueCommit={next => {
        setPending(null)
        if (next && /^#[0-9a-f]{6}$/i.test(next)) onChange(next.toUpperCase())
      }}>
      <ColorPickerInput />
      <ColorPickerContent className="display-color-picker-popup">
        <ColorPickerPanel>
          <div className="display-color-picker-label">预设颜色</div>
          <ColorPickerSwatches colors={presets} className="display-color-picker-swatches" />
          <ColorPickerArea className="display-color-picker-area" />
          <ColorPickerSlider channel="hue" />
        </ColorPickerPanel>
      </ColorPickerContent>
    </ColorPicker>
  </div>
}
