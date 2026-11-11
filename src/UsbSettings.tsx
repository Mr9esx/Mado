import { useState } from 'react'
import { Button } from './components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { Screen } from './Screen'
import { nextPageId, pagesFromSnapshot } from './pages'
import type { useUsbDevice } from './usbDevice'

export function UsbSettings({ usb, interval, onDismiss }: {
  usb: ReturnType<typeof useUsbDevice>; interval: number; onDismiss: () => void
}) {
  const [port, setPort] = useState(usb.status?.port ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const phase = usb.connection?.phase ?? 'waiting'
  const connected = phase === 'connected' || phase === 'syncing'
  const reconnecting = usb.status?.reconnecting === true
  const selected = usb.ports.includes(port) ? port : usb.ports[0] ?? ''
  const snapshot = usb.status?.preview
  const pages = pagesFromSnapshot(snapshot)
  const pageId = usb.status?.activePageId ?? pages[0].id
  const page = pages.find(candidate => candidate.id === pageId) ?? pages[0]
  const preview = snapshot?.appliedWidgets ? { ...snapshot, widgets: snapshot.appliedWidgets } : snapshot

  async function perform(command: () => Promise<void>) {
    if (busy) return
    setError('')
    setBusy(true)
    try { await command() }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }

  return <Dialog open onOpenChange={open => { if (!open) onDismiss() }}>
    <DialogContent className="settings-dialog">
      <DialogHeader>
        <DialogTitle>USB 设备</DialogTitle>
        <DialogDescription>设备使用已应用布局，并按设定间隔接收全部页面的读数。</DialogDescription>
      </DialogHeader>
      <div className="usb-settings">
        <fieldset disabled={busy}>
          <legend>连接设置</legend>
          <div className="usb-port">
            <Select value={selected} disabled={connected || reconnecting || !usb.ports.length} onValueChange={setPort}>
              <SelectTrigger size="sm" aria-label="USB 端口"><SelectValue placeholder="未找到 USB 设备" /></SelectTrigger>
              <SelectContent>{usb.ports.map(value => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent>
            </Select>
            <Button variant="outline" size="sm" onClick={() => void perform(usb.refreshPorts)}>刷新</Button>
            {connected || reconnecting
              ? <Button size="sm" variant="outline" onClick={() => void perform(usb.disconnect)}>{reconnecting ? '停止重连' : '断开'}</Button>
              : <Button size="sm" disabled={!selected}
                onClick={() => void perform(() => usb.connect(selected, interval))}>连接设备</Button>}
          </div>
        </fieldset>
        <div>
          <p className="usb-preview-label">设备页面 · 最近一次确认的读数</p>
          <Screen layout={page.layout} pages={pages} pageId={page.id} snapshot={preview}
            connection={usb.connection ?? { phase: 'waiting', hasLayout: false }}
            onPageChange={phase === 'connected' && pages.length > 1 ? () => {
              const next = nextPageId(pages, page.id, 1)
              if (next) void perform(() => usb.selectPage(next))
            } : undefined} />
        </div>
        {error && <p className="page-error" role="alert">{error}</p>}
        {reconnecting && phase !== 'failed' && <p className="page-error" role="status">正在重连。{usb.connection?.reason}</p>}
      </div>
    </DialogContent>
  </Dialog>
}
