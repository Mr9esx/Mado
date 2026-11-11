import { Check, Monitor, Moon, Sun } from 'lucide-react'
import { Button } from './components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from './components/reui/color-picker-primitives'
import { useAppearance } from './appearance'
import type { AppTheme } from './appearanceModel'

const themes = {
  light: { label: '浅色', icon: Sun },
  dark: { label: '深色', icon: Moon },
  system: { label: '跟随系统', icon: Monitor },
}

export function AppThemeButton() {
  const { appearance, update, ready, busy, error } = useAppearance()
  const { icon: ThemeIcon, label } = themes[appearance.appTheme]
  return <DropdownMenu>
    <DropdownMenuTrigger asChild>
      <Button size="sm" variant="ghost" type="button" aria-label="App 主题"
        title={error || `App 主题 · ${label}`} disabled={!ready || busy}><ThemeIcon size={15} />主题</Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" className="min-w-28" aria-label="App 主题">
      <DropdownMenuRadioGroup value={appearance.appTheme} onValueChange={value => void update({ appTheme: value as AppTheme })}>
        {Object.entries(themes).map(([value, { label: name, icon: Icon }]) =>
          <DropdownMenuRadioItem key={value} value={value}>
            <Icon size={14} />{name}{appearance.appTheme === value && <Check className="ml-auto" size={14} aria-hidden="true" />}
          </DropdownMenuRadioItem>)}
      </DropdownMenuRadioGroup>
      {error && <p className="page-error" role="alert">{error}</p>}
    </DropdownMenuContent>
  </DropdownMenu>
}
