import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { Snapshot } from './layout'

export type DisplayTransport = 'usb' | 'ble' | 'wifi'
export type WifiDisplay = { id: string; name: string; address: string | null }
type DevicePower = { source: 'external' | 'battery' | 'unknown'; batteryPercent: number | null; charging?: boolean }
export type DisplayConnectionStatus = {
  phase: 'waiting' | 'syncing' | 'connected' | 'disconnected' | 'failed'
  transport?: DisplayTransport
  deviceId?: string
  wifiProvisioning?: boolean
  port?: string
  activePageId?: string
  brightness?: number
  screenRotated?: boolean | null
  firmwareVersion?: string
  lastReadingsAppliedAtUnixMs?: number
  reason?: string
  hasLayout?: boolean
  reconnecting?: boolean
  preview?: Snapshot
  power?: DevicePower
}

export function useDisplayConnection() {
  const [status, setStatus] = useState<DisplayConnectionStatus>()
  const [ports, setPorts] = useState<string[]>([])
  const refreshPorts = async () => setPorts(await invoke<string[]>('list_usb_displays'))
  useEffect(() => {
    let mounted = true
    let eventSeen = false
    const events = listen<DisplayConnectionStatus>('display-connection-status', ({ payload }) => {
      eventSeen = true
      if (mounted) setStatus(payload)
    })
    void events.then(async () => {
      const value = await invoke<DisplayConnectionStatus>('get_display_connection_status')
      if (mounted && !eventSeen) setStatus(value)
    }).catch(() => {})
    void refreshPorts().catch(() => {})
    return () => {
      mounted = false
      void events.then(stop => stop()).catch(() => {})
    }
  }, [])
  const connection = status && {
    phase: status.phase, transport: status.transport,
    firmwareVersion: status.firmwareVersion, lastReadingsAt: status.lastReadingsAppliedAtUnixMs == null ? null : status.lastReadingsAppliedAtUnixMs / 1000,
    reason: status.reason, hasLayout: status.hasLayout,
    power: (status.power?.source === 'battery' || status.power?.charging === true)
      ? { source: 'battery' as const, percent: status.power.batteryPercent, charging: status.power.charging }
      : { source: status.power?.source === 'external' ? 'external' as const : 'unknown' as const },
  }
  return {
    status, connection, ports, refreshPorts,
    connectUsb: (port: string, intervalMs = 1000) => invoke<void>('connect_usb_display', {
      port, intervalMs, utcOffsetMinutes: -new Date().getTimezoneOffset(),
    }),
    connectBle: (deviceId: string, intervalMs: number) => invoke<void>('connect_ble_display', {
      deviceId, intervalMs, utcOffsetMinutes: -new Date().getTimezoneOffset(),
    }),
    scanWifi: () => invoke<WifiDisplay[]>('scan_wifi_displays'),
    connectWifi: (deviceId: string, address: string | null, intervalMs: number) => invoke<void>('connect_wifi_display', {
      deviceId, address, intervalMs, utcOffsetMinutes: -new Date().getTimezoneOffset(),
    }),
    provisionWifi: (ssid: string, password: string, intervalMs: number) => invoke<void>('provision_wifi_display', {
      ssid, password, intervalMs, utcOffsetMinutes: -new Date().getTimezoneOffset(),
    }),
    scanBle: () => invoke<{ id: string; name: string }[]>('scan_ble_displays'),
    disconnect: () => invoke<void>('disconnect_display'),
    selectPage: (pageId: string) => invoke<void>('select_display_page', { pageId }),
    setBrightness: (percent: number) => invoke<void>('set_display_brightness', { percent }),
    setRotation: (rotated: boolean) => invoke<void>('set_display_rotation', { rotated }),
  }
}
