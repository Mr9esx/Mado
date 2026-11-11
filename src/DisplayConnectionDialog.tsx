import { Bluetooth, Cpu, Monitor, Usb, Wifi } from 'lucide-react'
import { useId, useState } from 'react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from './components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './components/ui/tabs'
import { displayTransportLabels, type DisplayTransport, type WifiDisplay, type useDisplayConnection } from './displayConnection'
import { FirmwareFlashPanel } from './FirmwareFlashPanel'

const transportIcons = { usb: Usb, ble: Bluetooth, wifi: Wifi }
const transports: DisplayTransport[] = ['usb', 'ble', 'wifi']
const phaseLabels = {
  waiting: '未连接', syncing: '正在连接', connected: '已连接',
  disconnected: '已断开', failed: '连接失败', reconnecting: '正在重连',
}

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
  const [showFlash, setShowFlash] = useState(false)
  const [error, setError] = useState('')
  const setupFormId = useId()
  const targetId = useId()
  const status = device.status
  const phase = status?.phase ?? 'waiting'
  const reconnecting = status?.reconnecting === true
  const hasSession = reconnecting || phase === 'connected' || phase === 'syncing'
  const visiblePhase = reconnecting ? 'reconnecting' : phase
  const transport = hasSession ? status?.transport ?? mode : mode
  const canConfigureWifi = phase === 'connected' && transport !== 'wifi' && status?.wifiProvisioning
  const configuringWifi = setup && canConfigureWifi
  const wifi = wifiDevices.find(candidate => candidate.id === wifiId) ?? wifiDevices[0]
  const selectedPort = device.ports.includes(port) ? port : device.ports[0] ?? ''
  const selectedBle = bleDevices.some(candidate => candidate.id === bleId) ? bleId : bleDevices[0]?.id ?? ''
  const deviceName = status?.deviceId && /^[0-9a-f]{12}$/i.test(status.deviceId)
    ? `Mado-${status.deviceId.slice(-4).toUpperCase()}` : 'Mado'
  const endpoint = transport === 'usb' || transport === 'wifi' ? status?.port : undefined
  const connectionError = error || (phase === 'failed' && !reconnecting ? status?.reason : undefined)
  const HeaderIcon = showFlash ? Cpu : Monitor

  async function scanWifi() {
    setWifiDevices(await device.scanWifi())
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

  const discovery = {
    usb: {
      label: 'USB 串口', value: selectedPort, select: setPort, scan: device.refreshPorts,
      options: device.ports.map(value => ({ value, label: value })),
      hint: '用 USB 数据线连接开发板，然后刷新串口。',
      empty: '未找到 USB 设备',
    },
    ble: {
      label: '附近的蓝牙设备', value: selectedBle, select: setBleId,
      scan: async () => { setBleDevices(await device.scanBle()) },
      options: bleDevices.map(({ id, name }) => ({ value: id, label: name })),
      hint: '首次连接时，长按设备 BOOT 键，打开「蓝牙配对」后再扫描。',
      empty: '未发现 Mado',
    },
    wifi: {
      label: '局域网设备', value: wifi?.id ?? '', scan: scanWifi,
      select: (value: string) => { setWifiId(value); setAddress('') },
      options: wifiDevices.map(({ id, name, address }) => ({ value: id, label: address ? `${name} · ${address}` : name })),
      hint: '首次使用 Wi-Fi，请先通过 USB 或蓝牙连接设备，再配置网络。',
      empty: '未发现已配置的设备',
    },
  }[transport]

  async function connect() {
    if (transport === 'usb') await device.connectUsb(selectedPort, interval)
    else if (transport === 'ble') await device.connectBle(selectedBle, Math.max(interval, 2000))
    else if (wifi) await device.connectWifi(wifi.id, address.trim() || wifi.address, interval)
  }

  return <Dialog open onOpenChange={open => { if (!open && !(showFlash && busy)) onDismiss() }}>
    <DialogContent className="settings-dialog device-modal" showCloseButton={!(showFlash && busy)}>
      <DialogHeader className="device-modal-header">
        <div className="device-modal-icon"><HeaderIcon size={20} aria-hidden="true" /></div>
        <div className="min-w-0 flex-1">
          <DialogTitle>{showFlash ? '烧录固件' : '设备连接'}</DialogTitle>
          <DialogDescription>LILYGO T-Display-C5 V1.0</DialogDescription>
        </div>
        {!showFlash && <span className="device-modal-status" role="status" data-phase={visiblePhase}>
          {phaseLabels[visiblePhase]}
        </span>}
      </DialogHeader>
      {showFlash ? <FirmwareFlashPanel device={device} initialPort={device.ports.includes(port) ? port : ''} busy={busy} setBusy={setBusy}
        onBack={() => { setMode('usb'); setShowFlash(false) }} /> : <>
        <Tabs className="device-modal-layout" value={transport} orientation="vertical" activationMode="manual"
          onValueChange={value => {
            if (busy || hasSession) return
            setMode(value as DisplayTransport)
            setError('')
            setSetup(false)
            if (value === 'wifi') void perform(scanWifi)
          }}>
          <TabsList variant="button" size="sm" className="device-modal-nav" aria-label="连接方式">
            {transports.map(id => {
              const Icon = transportIcons[id]
              return <TabsTrigger key={id} value={id} disabled={id !== transport && (busy || hasSession)}>
                <Icon aria-hidden="true" />{displayTransportLabels[id]}
              </TabsTrigger>
            })}
          </TabsList>
          <TabsContent value={transport} className="device-modal-pane" aria-busy={busy}>
            {configuringWifi ? <form id={setupFormId} onSubmit={event => { event.preventDefault(); void perform(configureWifi) }}>
              <h3 className="device-modal-pane-title">配置 Wi-Fi</h3>
              <fieldset disabled={busy} className="device-modal-fields">
                <label className="device-modal-field">
                  Wi-Fi 网络名
                  <Input variant="sm" value={ssid} onChange={event => setSsid(event.target.value)}
                    autoFocus required maxLength={32} autoComplete="off" spellCheck={false} />
                </label>
                <label className="device-modal-field">
                  Wi-Fi 密码
                  <Input variant="sm" type="password" value={password} onChange={event => setPassword(event.target.value)}
                    maxLength={64} autoComplete="off" placeholder="开放网络可留空" />
                </label>
              </fieldset>
              {busy && <p className="device-connection-hint" role="status">正在等待设备加入网络。</p>}
            </form> : hasSession ? <>
              <h3 className="device-modal-pane-title">当前设备</h3>
              <div className="device-modal-device">
                <Monitor size={18} aria-hidden="true" />
                <div className="min-w-0">
                  <p className="font-medium">{deviceName}</p>
                  {(endpoint || status?.firmwareVersion) && <p className="device-connection-hint break-all">
                    {[endpoint, status?.firmwareVersion && `固件 ${status.firmwareVersion}`].filter(Boolean).join(' · ')}
                  </p>}
                </div>
              </div>
              {reconnecting && status?.reason && <p className="device-connection-hint">{status.reason}</p>}
              {transport === 'ble' && phase === 'syncing' && <p className="device-connection-hint" role="status">
                若系统询问配对码，请输入设备屏上显示的六位数字。
              </p>}
            </> : <>
              <label className="device-modal-pane-title" htmlFor={targetId}>{discovery.label}</label>
              <div className="device-modal-row">
                <Select value={discovery.value} disabled={busy || !discovery.options.length} onValueChange={discovery.select}>
                  <SelectTrigger id={targetId} size="sm"><SelectValue placeholder={discovery.empty} /></SelectTrigger>
                  <SelectContent>{discovery.options.map(({ value, label }) =>
                    <SelectItem key={value} value={value}>{label}</SelectItem>)}</SelectContent>
                </Select>
                <Button variant="outline" size="sm" disabled={busy} onClick={() => void perform(discovery.scan)}>
                  {transport === 'usb' ? '刷新' : '扫描'}
                </Button>
              </div>
              {transport === 'wifi' && wifi && <Input variant="sm" aria-label="设备 IPv4 地址"
                placeholder="设备 IP（可选，默认自动发现）" inputMode="decimal" disabled={busy}
                value={address} onChange={event => setAddress(event.target.value)} />}
              {!discovery.options.length && <p className="device-connection-hint">{discovery.hint}</p>}
            </>}
            {connectionError && <p className="device-modal-feedback" role="alert" data-kind="error">{connectionError}</p>}
          </TabsContent>
        </Tabs>
        <footer className="device-modal-footer">
          <Button variant="ghost" size="sm" disabled={busy || configuringWifi} onClick={() => setShowFlash(true)}>
            <Cpu aria-hidden="true" />烧录固件
          </Button>
          <div className="device-modal-actions">
            {configuringWifi ? <>
              <Button variant="outline" size="sm" disabled={busy} onClick={() => {
                setSetup(false); setPassword(''); setError('')
              }}>取消</Button>
              <Button type="submit" form={setupFormId} size="sm" disabled={busy || !ssid}>
                {busy ? '正在配网…' : '配置并连接'}
              </Button>
            </> : hasSession ? <>
              {canConfigureWifi && <Button variant="outline" size="sm" disabled={busy} onClick={() => {
                setSetup(true); setPassword(''); setError('')
              }}>配置 Wi-Fi</Button>}
              <Button variant="outline" size="sm" disabled={busy} onClick={() => void perform(device.disconnect)}>
                {reconnecting ? '停止重连' : '断开连接'}
              </Button>
            </> : <Button size="sm" disabled={busy || !discovery.value} onClick={() => void perform(connect)}>连接设备</Button>}
          </div>
        </footer>
      </>}
    </DialogContent>
  </Dialog>
}
