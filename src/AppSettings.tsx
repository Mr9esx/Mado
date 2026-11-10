import { Monitor, Moon, Sun } from 'lucide-react'
import { Button } from './components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './components/ui/tabs'
import { CodexSettingsPanel } from './CodexSettings'
import type { CodexStatus } from './codex'
import { useAppearance } from './appearance'

export function AppearanceSettings() {
  const { appearance, update, ready, busy, error } = useAppearance()
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
      <h3 id="display-theme-title">桌面控件主题</h3>
      <p>同步到画布预览和组件缩略图。</p>
      <div className="display-theme-options" role="group" aria-label="桌面控件主题">
        {([{ value: 'dark', label: '深色' }, { value: 'light', label: '浅色' }] as const).map(({ value, label }) =>
          <Button key={value} variant="outline" className="display-theme-option" aria-label={`桌面控件主题：${label}`}
            aria-pressed={appearance.displayTheme === value} disabled={!ready || busy} onClick={() => void update({ displayTheme: value })}>
            <span className="theme-sample" data-display-theme={value} aria-hidden="true"><i /><i /><i /><i /></span>
            <span>{label}</span>
          </Button>)}
      </div>
    </section>
    <p className="appearance-save-state" role="status">{busy ? '正在保存…' : ready ? '更改即时保存，重启后保留。' : '正在读取外观设置…'}</p>
    {error && <p className="page-error" role="alert">{error}</p>}
  </div>
}

export function AppSettings({ initialTab = 'appearance', status, error, onDismiss }: {
  initialTab?: 'appearance' | 'source'; status?: CodexStatus; error: string; onDismiss: () => void
}) {
  return <Dialog open onOpenChange={open => { if (!open) onDismiss() }}>
    <DialogContent className="settings-dialog">
      <DialogHeader><DialogTitle>设置</DialogTitle><DialogDescription className="sr-only">调整外观与数据源连接。</DialogDescription></DialogHeader>
      <Tabs defaultValue={initialTab}>
        <TabsList size="sm" variant="line" aria-label="设置分类">
          <TabsTrigger value="appearance">外观</TabsTrigger><TabsTrigger value="source">数据源</TabsTrigger>
        </TabsList>
        <TabsContent value="appearance"><AppearanceSettings /></TabsContent>
        <TabsContent value="source"><CodexSettingsPanel status={status} error={error} /></TabsContent>
      </Tabs>
    </DialogContent>
  </Dialog>
}
