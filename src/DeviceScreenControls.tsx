import { RotateCw, Sun } from 'lucide-react'
import { useState } from 'react'
import { Button } from './components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from './components/reui/color-picker-primitives'
import type { useDisplayConnection } from './displayConnection'

export function DeviceScreenControls({ device }: { device: ReturnType<typeof useDisplayConnection> }) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [brightnessDraft, setBrightnessDraft] = useState<number>()
  const connected = device.status?.phase === 'connected'
  const rotated = connected ? device.status?.screenRotated : undefined
  const confirmedBrightness = connected ? device.status?.brightness : undefined
  const brightness = brightnessDraft ?? confirmedBrightness ?? 70

  async function apply(command: () => Promise<void>) {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      await command()
    } catch (reason) {
      setError(String(reason))
    } finally {
      setBusy(false)
    }
  }

  return <div className="device-screen-controls" role="group" aria-label="实体屏幕">
    <Popover onOpenChange={open => { if (open) setError('') }}>
      <PopoverTrigger asChild>
        <Button size="xs" mode="icon" variant="outline" type="button" aria-label="设备方向"
          title={rotated == null ? '设备方向' : `设备方向 · ${rotated ? '180°' : '0°'}`}>
          <RotateCw size={14} aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-56" aria-label="设备方向">
        <p className="mb-2 text-xs font-semibold">设备方向</p>
        {rotated == null
          ? <p className="text-xs text-muted-foreground">{connected ? '当前固件未提供方向切换。' : '连接设备后可切换方向。'}</p>
          : <div className="flex gap-2" role="group" aria-label="实体屏幕方向" aria-busy={busy}>
            <Button size="xs" type="button" variant={rotated ? 'outline' : 'secondary'} aria-pressed={!rotated}
              disabled={busy} onClick={() => { if (rotated) void apply(() => device.setRotation(false)) }}>正向 0°</Button>
            <Button size="xs" type="button" variant={rotated ? 'secondary' : 'outline'} aria-pressed={rotated}
              disabled={busy} onClick={() => { if (!rotated) void apply(() => device.setRotation(true)) }}>反向 180°</Button>
          </div>}
        {error && <p className="page-error" role="alert">{error}</p>}
      </PopoverContent>
    </Popover>
    <Popover onOpenChange={open => {
      if (open) {
        setBrightnessDraft(undefined)
        setError('')
      }
    }}>
      <PopoverTrigger asChild>
        <Button size="xs" mode="icon" variant="outline" type="button" aria-label="背光亮度"
          title={confirmedBrightness == null ? '背光亮度' : `背光亮度 · ${confirmedBrightness}%`}>
          <Sun size={14} aria-hidden="true" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-56" aria-label="背光亮度">
        <p className="mb-2 text-xs font-semibold">背光亮度</p>
        {connected ? <fieldset className="grid min-w-0 gap-2" disabled={busy}>
          <div className="flex items-center gap-2">
            <input type="range" aria-label="背光亮度" min={0} max={100} value={brightness}
              className="min-w-0 flex-1" onChange={event => setBrightnessDraft(event.target.valueAsNumber)} />
            <span className="w-10 text-right text-xs tabular-nums">{brightness}%</span>
          </div>
          <Button size="xs" type="button" variant="outline" className="justify-self-end"
            disabled={brightness === confirmedBrightness} onClick={() => void apply(() => device.setBrightness(brightness))}>应用亮度</Button>
        </fieldset> : <p className="text-xs text-muted-foreground">连接设备后可调整背光。</p>}
        {error && <p className="page-error" role="alert">{error}</p>}
      </PopoverContent>
    </Popover>
  </div>
}
