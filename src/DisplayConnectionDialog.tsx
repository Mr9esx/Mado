import { useState } from 'react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { Screen } from './Screen'
import { nextPageId, pagesFromSnapshot } from './pages'
import type { DisplayTransport, WifiDisplay, useDisplayConnection } from './displayConnection'

export function DisplayConnectionDialog({ device, interval, onDismiss }: {
  device: ReturnType<typeof useDisplayConnection>; interval: number; onDismiss: () => void
}) {
  const [mode, setMode] = useState<DisplayTransport>(device.status?.transport ?? 'usb')
  const [wifiDevices, setWifiDevices] = useState<WifiDisplay[]>([])
  const [wifiId, setWifiId] = useState('')
  const [address, setAddress] = useState('')
  const [setup, setSetup] = useState(false)
  const [ssid, setSsid] = useState('')
  const [password, setPassword] = useState('')
  const [bleDevices, setBleDevices] = useState<{ id: string; name: string }[]>([])
  const [bleId, setBleId] = useState('')
  const [port, setPort] = useState(device.status?.port ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const phase = device.connection?.phase ?? 'waiting'
  const connected = phase === 'connected' || phase === 'syncing'
  const reconnecting = device.status?.reconnecting === true
  const transport = connected || reconnecting ? device.status?.transport ?? mode : mode
  const canConfigureWifi = phase === 'connected' && transport !== 'wifi' && device.status?.wifiProvisioning
  const wifi = wifiDevices.find(candidate => candidate.id === wifiId) ?? wifiDevices[0]
  const selected = device.ports.includes(port) ? port : device.ports[0] ?? ''
  const selectedBle = bleDevices.some(candidate => candidate.id === bleId) ? bleId : bleDevices[0]?.id ?? ''
  const snapshot = device.status?.preview
  const pages = pagesFromSnapshot(snapshot)
  const pageId = device.status?.activePageId ?? pages[0].id
  const page = pages.find(candidate => candidate.id === pageId) ?? pages[0]
  const preview = snapshot?.appliedWidgets ? { ...snapshot, widgets: snapshot.appliedWidgets } : snapshot

  async function scanWifi() {
    const found = await device.scanWifi()
    setWifiDevices(found)
    if (!found.length) setError('请先通过蓝牙或 USB 连接设备，再配置 Wi-Fi。')
  }

  async function configureWifi() {
    const secret = password
    setPassword('')
    await device.provisionWifi(ssid, secret, interval)
    setSetup(false)
    setMode('wifi')
    setAddress('')
  }

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
        <DialogTitle>设备连接</DialogTitle>
        <DialogDescription>设备使用已应用布局，并按设定间隔接收全部页面的读数。</DialogDescription>
      </DialogHeader>
      <div className="usb-settings">
        <fieldset disabled={busy}>
          <legend>连接设置</legend>
          <Select value={transport} disabled={busy || connected || reconnecting}
            onValueChange={value => {
              setMode(value as DisplayTransport)
              setError('')
              setSetup(false)
              if (value === 'wifi') void perform(scanWifi)
            }}>
            <SelectTrigger size="sm" aria-label="连接方式"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="usb">USB</SelectItem>
              <SelectItem value="ble">蓝牙</SelectItem>
              <SelectItem value="wifi">Wi-Fi</SelectItem>
            </SelectContent>
          </Select>
          {transport === 'ble' && !connected && !reconnecting &&
            <p className="usb-preview-label">首次连接时，在设备上长按 BOOT，打开「蓝牙配对」。</p>}
          <div className="usb-port">
            {transport === 'usb' ? <>
              <Select value={selected} disabled={connected || reconnecting || !device.ports.length} onValueChange={setPort}>
                <SelectTrigger size="sm" aria-label="USB 端口"><SelectValue placeholder="未找到 USB 设备" /></SelectTrigger>
                <SelectContent>{device.ports.map(value => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent>
              </Select>
              <Button variant="outline" size="sm" disabled={connected || reconnecting}
                onClick={() => void perform(device.refreshPorts)}>刷新</Button>
            </> : transport === 'ble' ? <>
              <Select value={selectedBle} disabled={connected || reconnecting || !bleDevices.length} onValueChange={setBleId}>
                <SelectTrigger size="sm" aria-label="附近的蓝牙设备"><SelectValue placeholder={connected ? 'Mado' : '扫描附近的 Mado'} /></SelectTrigger>
                <SelectContent>{bleDevices.map(({ id, name }) => <SelectItem key={id} value={id}>{name}</SelectItem>)}</SelectContent>
              </Select>
              <Button variant="outline" size="sm" disabled={connected || reconnecting}
                onClick={() => void perform(async () => {
                  const found = await device.scanBle()
                  setBleDevices(found)
                  if (!found.length) setError('未发现 Mado，请检查设备上的配对页面。')
                })}>扫描</Button>
            </> : <>
              <Select value={wifi?.id ?? ''} disabled={connected || reconnecting || !wifiDevices.length}
                onValueChange={value => { setWifiId(value); setAddress('') }}>
                <SelectTrigger size="sm" aria-label="已配置 Wi-Fi 的设备">
                  <SelectValue placeholder={connected ? 'Mado' : '未选择已配置的设备'} />
                </SelectTrigger>
                <SelectContent>{wifiDevices.map(({ id, name, address }) =>
                  <SelectItem key={id} value={id}>{address ? `${name} · ${address}` : name}</SelectItem>)}</SelectContent>
              </Select>
              <Button variant="outline" size="sm" disabled={connected || reconnecting}
                onClick={() => void perform(scanWifi)}>扫描</Button>
            </>}
            {connected || reconnecting
              ? <Button size="sm" variant="outline" onClick={() => void perform(device.disconnect)}>{reconnecting ? '停止重连' : '断开'}</Button>
              : <Button size="sm" disabled={transport === 'usb' ? !selected : transport === 'ble' ? !selectedBle : !wifi}
                onClick={() => void perform(() => transport === 'usb' ? device.connectUsb(selected, interval)
                  : transport === 'ble' ? device.connectBle(selectedBle, Math.max(interval, 2000))
                  : device.connectWifi(wifi.id, address.trim() || wifi.address, interval))}>连接设备</Button>}
          </div>
          {transport === 'wifi' && !connected && !reconnecting && wifi &&
            <Input aria-label="设备 IPv4 地址" placeholder="设备 IP（可选，默认自动发现）"
              inputMode="decimal" value={address} onChange={event => setAddress(event.target.value)} />}
          {canConfigureWifi && <div>
            <Button variant="outline" size="sm" onClick={() => { setSetup(!setup); setPassword(''); setError('') }}>
              {setup ? '返回设备预览' : '配置 Wi-Fi'}
            </Button>
          </div>}
          {transport === 'ble' && phase === 'syncing' && <p className="usb-preview-label" role="status">
            正在连接。若系统询问配对码，请输入设备屏上显示的六位数字。
          </p>}
        </fieldset>
        {setup && canConfigureWifi ? <form onSubmit={event => { event.preventDefault(); void perform(configureWifi) }}>
          <fieldset disabled={busy}>
            <label className="grid gap-1 text-sm">
              Wi-Fi 网络名
              <Input value={ssid} onChange={event => setSsid(event.target.value)}
                required maxLength={32} autoComplete="off" spellCheck={false} />
            </label>
            <label className="grid gap-1 text-sm">
              Wi-Fi 密码
              <Input type="password" value={password} onChange={event => setPassword(event.target.value)}
                maxLength={64} autoComplete="off" placeholder="开放网络可留空" />
            </label>
            <div>
              <Button type="submit" size="sm" disabled={!ssid}>{busy ? '正在配网…' : '配置并连接'}</Button>
            </div>
          </fieldset>
          {busy && <p className="usb-preview-label" role="status">正在等待设备加入网络。</p>}
        </form> : <div>
          <p className="usb-preview-label">设备页面 · 最近一次确认的读数</p>
          <Screen layout={page.layout} pages={pages} pageId={page.id} snapshot={preview}
            connection={device.connection ?? { phase: 'waiting', hasLayout: false }}
            onPageChange={phase === 'connected' && pages.length > 1 ? () => {
              const next = nextPageId(pages, page.id, 1)
              if (next) void perform(() => device.selectPage(next))
            } : undefined} />
        </div>}
        {(error || phase === 'failed') && <p className="page-error" role="alert">{error || device.connection?.reason}</p>}
        {reconnecting && phase !== 'failed' && <p className="page-error" role="status">正在重连。{device.connection?.reason}</p>}
      </div>
    </DialogContent>
  </Dialog>
}
