import { Channel, invoke } from '@tauri-apps/api/core'
import { Archive, ArrowLeft, ChevronRight, Usb } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Button } from './components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './components/ui/select'
import type { useDisplayConnection } from './displayConnection'

type PackageInfo = { id: string; fileName: string; version: string; mode: 'install' | 'upgrade' }

const progressLabels = {
  connecting: '正在连接下载模式…',
  checking: '正在检查设备分区表…',
  writing: '正在写入镜像…',
  verifying: '正在校验写入结果…',
  restarting: '正在重启设备…',
}
type Progress = { phase: keyof typeof progressLabels; percent: number }

export function FirmwareFlashPanel({ device, initialPort, busy, setBusy, onBack }: {
  device: ReturnType<typeof useDisplayConnection>
  initialPort: string
  busy: boolean
  setBusy: (busy: boolean) => void
  onBack: () => void
}) {
  const [port, setPort] = useState(initialPort)
  const [firmware, setFirmware] = useState<PackageInfo | null>(null)
  const [manualBoot, setManualBoot] = useState(false)
  const [progress, setProgress] = useState<Progress | null>(null)
  const [complete, setComplete] = useState(false)
  const [error, setError] = useState('')
  // Retain an explicitly selected port when it disappears; never silently flash another device.
  const selected = port ? (device.ports.includes(port) ? port : '') :
    device.ports.length === 1 ? device.ports[0] : ''

  useEffect(() => {
    void device.refreshPorts().catch(reason => setError(String(reason)))
  }, [])

  async function perform(command: () => Promise<void>) {
    setBusy(true)
    setError('')
    try { await command() }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }

  function choosePackage() {
    void perform(async () => {
      try {
        const info = await invoke<PackageInfo | null>('choose_firmware_package')
        if (info) {
          setFirmware(info)
          setComplete(false)
          setProgress(null)
        }
      } catch (reason) {
        setFirmware(null)
        setComplete(false)
        setProgress(null)
        throw reason
      }
    })
  }

  function flash() {
    if (!firmware || !selected) return
    setPort(selected)
    setComplete(false)
    setProgress({ phase: 'connecting', percent: 0 })
    void perform(async () => {
      const updates = new Channel<Progress>()
      updates.onmessage = setProgress
      try {
        await invoke('flash_firmware_package', {
          packageId: firmware.id, port: selected, manualBoot, progress: updates,
        })
        setComplete(true)
      } finally {
        updates.onmessage = () => {}
        setProgress(null)
        await device.refreshPorts().catch(() => {})
      }
    })
  }

  return <>
    <div className="device-flash-body" aria-busy={busy}>
      <fieldset disabled={busy} className="device-flash-card" aria-label="烧录设置">
        <div className="device-flash-row">
          <Archive size={17} aria-hidden="true" />
          <div className="device-flash-value">
            <p className="device-modal-pane-title">本地烧录包</p>
            {firmware ? <>
              <p className="truncate text-sm" title={firmware.fileName}>{firmware.fileName}</p>
              <p className="device-connection-hint">
                {firmware.version} · {firmware.mode === 'install'
                  ? '首次安装，将替换设备原有固件'
                  : '应用升级，保留字库和设备配置'}
              </p>
            </> : <p className="device-connection-hint">选择自行下载的 Mado ZIP 烧录包。</p>}
          </div>
          <Button variant="outline" size="sm" onClick={choosePackage}>{firmware ? '更换 ZIP' : '选择 ZIP'}</Button>
        </div>
        <div className="device-flash-row">
          <Usb size={17} aria-hidden="true" />
          <div className="device-flash-value">
            <p className="device-modal-pane-title">目标设备</p>
            <div className="device-modal-row">
              <Select value={selected} disabled={busy || !device.ports.length} onValueChange={setPort}>
                <SelectTrigger size="sm" aria-label="烧录串口">
                  <SelectValue placeholder={device.ports.length ? '选择 USB 串口' : '未找到 USB 设备'} />
                </SelectTrigger>
                <SelectContent>{device.ports.map(value => <SelectItem key={value} value={value}>{value}</SelectItem>)}</SelectContent>
              </Select>
              <Button variant="outline" size="sm" onClick={() => void perform(device.refreshPorts)}>刷新</Button>
            </div>
          </div>
        </div>
      </fieldset>
      <details className="device-flash-help">
        <summary><ChevronRight size={14} aria-hidden="true" />无法进入下载模式？</summary>
        <p>断开开发板电源，按住 BOOT 键后接入 USB，松开按键并刷新端口。</p>
        <label className="flex items-center gap-2">
          <input type="checkbox" disabled={busy} checked={manualBoot} onChange={event => setManualBoot(event.target.checked)} />
          已手动进入下载模式
        </label>
      </details>
      {progress && <div className="device-modal-feedback">
        <div className="flex justify-between gap-3" role="status">
          <span>{progressLabels[progress.phase]} 请保持 USB 连接。</span>
          <span className="tabular-nums">{progress.percent}%</span>
        </div>
        <progress className="w-full accent-primary" aria-label="烧录进度" max={100} value={progress.percent} />
      </div>}
      {error && <p className="device-modal-feedback" role="alert" data-kind="error">{error}</p>}
      {complete && <p className="device-modal-feedback" role="status" data-kind="success">烧录完成。返回设备连接即可连接屏幕。</p>}
    </div>
    <footer className="device-modal-footer">
      <Button variant="ghost" size="sm" disabled={busy} onClick={onBack}>
        <ArrowLeft aria-hidden="true" />返回连接
      </Button>
      <Button size="sm" disabled={busy || complete || !firmware || !selected} onClick={flash}>
        {progress ? '烧录中…' : '开始烧录'}
      </Button>
    </footer>
  </>
}
