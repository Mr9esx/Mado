import { Monitor, Moon, Sun } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './components/ui/tabs'
import { CodexSettingsPanel } from './CodexSettings'
import type { CodexStatus } from './codex'
import { useAppearance } from './appearance'
import { ColorField } from './CardSettings'
import { defaultDisplayTheme, type DisplayTheme, type Palette } from './layout'
import type { useUsbDevice } from './usbDevice'

type ScreenThemeEditor = { value: DisplayTheme; onChange: (theme: DisplayTheme) => void }
type DeviceSettingsProps = { usb: ReturnType<typeof useUsbDevice>; interval: number; onIntervalChange: (value: number) => void }

export function AppearanceSettings({ screenTheme }: { screenTheme?: ScreenThemeEditor } = {}) {
  const { appearance, update, ready, busy, error } = useAppearance()
  const activeTheme = screenTheme?.value.active ?? appearance.displayTheme
  return <div className="appearance-settings">
    <section aria-labelledby="app-theme-title">
      <h3 id="app-theme-title">App 外观</h3>
      <p>用于编辑器、设置和采集器窗口。</p>
      <div className="appearance-options" role="group" aria-label="App 外观">
        {([{ value: 'light', label: '浅色', icon: Sun }, { value: 'dark', label: '深色', icon: Moon },
          { value: 'system', label: '跟随系统', icon: Monitor }] as const).map(({ value, label, icon: Icon }) =>
          <Button key={value} variant={appearance.appTheme === value ? 'secondary' : 'outline'} size="sm"
            aria-label={`App 外观：${label}`} aria-pressed={appearance.appTheme === value} disabled={!ready || busy}
            onClick={() => void update({ appTheme: value })}><Icon size={15} />{label}</Button>)}
      </div>
    </section>
    <section aria-labelledby="display-theme-title">
      <h3 id="display-theme-title">{screenTheme ? '屏幕外观' : '桌面控件主题'}</h3>
      <p>{screenTheme ? '预览使用草稿主题，应用布局后同步到显示端。' : '同步到画布预览和组件缩略图。'}</p>
      <div className="display-theme-options" role="group" aria-label={screenTheme ? '屏幕主题' : '桌面控件主题'}>
        {([{ value: 'dark', label: '深色' }, { value: 'light', label: '浅色' }] as const).map(({ value, label }) =>
          <Button key={value} variant="outline" className="display-theme-option" aria-label={`桌面控件主题：${label}`}
            aria-pressed={activeTheme === value} disabled={!ready || busy}
            onClick={() => screenTheme ? screenTheme.onChange({ ...screenTheme.value, active: value }) : void update({ displayTheme: value })}>
            <span className="theme-sample" data-display-theme={value} aria-hidden="true"><i /><i /><i /><i /></span>
            <span>{label}</span>
          </Button>)}
      </div>
      {screenTheme && <div className="display-palette">
        {([['background', '屏幕背景'], ['surface', '卡片与状态栏底色'], ['text', '文字和图标']] as [keyof Palette, string][]).map(([key, label]) =>
          <ColorField key={key} label={label} value={screenTheme.value[activeTheme][key]}
            onChange={value => screenTheme.onChange({
              ...screenTheme.value,
              [activeTheme]: { ...screenTheme.value[activeTheme], [key]: value },
            })} />)}
        <Button variant="outline" size="sm"
          onClick={() => screenTheme.onChange({ ...screenTheme.value, [activeTheme]: defaultDisplayTheme[activeTheme] })}>恢复此主题的默认色</Button>
      </div>}
    </section>
    <p className="appearance-save-state" role="status">{busy ? '正在保存…' : ready
      ? screenTheme ? 'App 外观即时保存，屏幕外观随布局应用。' : '更改即时保存，重启后保留。'
      : '正在读取外观设置…'}</p>
    {error && <p className="page-error" role="alert">{error}</p>}
  </div>
}

function DeviceSettings({ usb, interval, onIntervalChange }: DeviceSettingsProps) {
  const [intervalText, setIntervalText] = useState(String(interval))
  const [brightness, setBrightness] = useState(usb.status?.brightness ?? 70)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const connected = usb.status?.phase === 'connected'
  const rotated = usb.status?.screenRotated
  const intervalValue = Number(intervalText)
  const validInterval = Number.isInteger(intervalValue) && intervalValue >= 500 && intervalValue <= 60_000
  useEffect(() => {
    if (usb.status?.brightness != null) setBrightness(usb.status.brightness)
  }, [usb.status?.brightness])
  async function perform(command: () => Promise<void>) {
    if (busy) return
    setBusy(true)
    setError('')
    try { await command() }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }
  return <div className="appearance-settings device-settings">
    <section aria-labelledby="device-push-title">
      <h3 id="device-push-title">数据推送</h3>
      <p id="device-push-hint">500–60000 毫秒，下次连接设备时生效。</p>
      <div className="device-setting-row">
        <label htmlFor="device-push-interval">推送间隔（毫秒）</label>
        <Input id="device-push-interval" variant="sm" type="number" min={500} max={60000} step={1}
          aria-describedby="device-push-hint" aria-invalid={!validInterval} value={intervalText} onChange={event => {
            const { value, valueAsNumber } = event.target
            setIntervalText(value)
            if (Number.isInteger(valueAsNumber) && valueAsNumber >= 500 && valueAsNumber <= 60_000) onIntervalChange(valueAsNumber)
          }} />
      </div>
    </section>
    <section aria-labelledby="device-screen-title">
      <h3 id="device-screen-title">实体屏幕</h3>
      {!connected ? <p>连接设备后可调整亮度和方向。</p> : <fieldset className="device-screen-settings" disabled={busy}>
        <div className="device-setting-row">
          <label htmlFor="device-brightness">背光亮度</label>
          <input id="device-brightness" type="range" min={0} max={100} value={brightness}
            onChange={event => setBrightness(event.target.valueAsNumber)} />
          <span>{brightness}%</span>
          <Button variant="outline" size="sm" onClick={() => void perform(() => usb.setBrightness(brightness))}>应用亮度</Button>
        </div>
        {rotated != null && <Button variant={rotated ? 'secondary' : 'outline'} size="sm" aria-pressed={rotated}
          onClick={() => void perform(() => usb.setRotation(!rotated))}>屏幕反转（180°）</Button>}
      </fieldset>}
    </section>
    {error && <p className="page-error" role="alert">{error}</p>}
  </div>
}

export function AppSettings({ initialTab = 'appearance', status, error, onDismiss, screenTheme, device }: {
  initialTab?: 'appearance' | 'source'; status?: CodexStatus; error: string; onDismiss: () => void;
  screenTheme?: ScreenThemeEditor; device?: DeviceSettingsProps
}) {
  return <Dialog open onOpenChange={open => { if (!open) onDismiss() }}>
    <DialogContent className="settings-dialog">
      <DialogHeader><DialogTitle>设置</DialogTitle><DialogDescription className="sr-only">调整外观、设备和数据源。</DialogDescription></DialogHeader>
      <Tabs defaultValue={initialTab}>
        <TabsList size="sm" variant="line" aria-label="设置分类">
          <TabsTrigger value="appearance">外观</TabsTrigger>
          {device && <TabsTrigger value="device">设备</TabsTrigger>}
          <TabsTrigger value="source">数据源</TabsTrigger>
        </TabsList>
        <TabsContent value="appearance"><AppearanceSettings screenTheme={screenTheme} /></TabsContent>
        {device && <TabsContent value="device"><DeviceSettings {...device} /></TabsContent>}
        <TabsContent value="source"><CodexSettingsPanel status={status} error={error} /></TabsContent>
      </Tabs>
    </DialogContent>
  </Dialog>
}
