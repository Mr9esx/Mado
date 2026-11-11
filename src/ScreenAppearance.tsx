import { ColorField } from './ColorField'
import { Button } from './components/ui/button'
import { defaultDisplayTheme, type DisplayTheme } from './layout'

const themes = [['dark', '深色'], ['light', '浅色']] as const
const colors = [['background', '屏幕背景'], ['surface', '卡片与状态栏底色'], ['text', '文字和图标']] as const

export function ScreenAppearance({ theme, onChange }: {
  theme: DisplayTheme; onChange: (theme: DisplayTheme) => void
}) {
  const active = theme.active
  const hasCustomColors = colors.some(([key]) => theme[active][key] !== defaultDisplayTheme[active][key])
  return <div className="screen-appearance">
    <div className="screen-theme-row">
      <div className="display-theme-options" role="group" aria-label="屏幕主题">
        {themes.map(([value, label]) => <Button key={value} size="xs" type="button"
          variant={active === value ? 'secondary' : 'outline'} aria-pressed={active === value}
          onClick={() => onChange({ ...theme, active: value })}>
          <span className="theme-swatch" aria-hidden="true" style={{ background: theme[value].background }} />{label}
        </Button>)}
      </div>
      <Button variant="ghost" size="xs" type="button" disabled={!hasCustomColors}
        title="恢复当前主题的默认色"
        onClick={() => onChange({ ...theme, [active]: defaultDisplayTheme[active] })}>恢复默认色</Button>
    </div>
    <div className="display-palette" role="group" aria-label="屏幕颜色">
      {colors.map(([key, label]) => <ColorField key={key} label={label} value={theme[active][key]}
        onChange={value => onChange({ ...theme, [active]: { ...theme[active], [key]: value } })} />)}
    </div>
  </div>
}
