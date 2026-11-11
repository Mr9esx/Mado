import { useState } from 'react'
import { Input } from './components/ui/input'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'

export function AppSettings({ onDismiss, interval, onIntervalChange }: {
  onDismiss: () => void; interval: number; onIntervalChange: (value: number) => void
}) {
  const [intervalText, setIntervalText] = useState(String(interval))
  const intervalValue = Number(intervalText)
  const validInterval = Number.isInteger(intervalValue) && intervalValue >= 500 && intervalValue <= 60_000

  return <Dialog open onOpenChange={open => { if (!open) onDismiss() }}>
    <DialogContent className="settings-dialog">
      <DialogHeader>
        <DialogTitle>设备设置</DialogTitle>
        <DialogDescription>推送间隔为 500–60000 毫秒，下次连接设备时生效。</DialogDescription>
      </DialogHeader>
      <div className="device-setting-row">
        <label htmlFor="device-push-interval">推送间隔（毫秒）</label>
        <Input id="device-push-interval" variant="sm" type="number" min={500} max={60000} step={1}
          aria-invalid={!validInterval} value={intervalText} onChange={event => {
            const { value, valueAsNumber } = event.target
            setIntervalText(value)
            if (Number.isInteger(valueAsNumber) && valueAsNumber >= 500 && valueAsNumber <= 60_000) onIntervalChange(valueAsNumber)
          }} />
      </div>
    </DialogContent>
  </Dialog>
}
