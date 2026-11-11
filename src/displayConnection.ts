import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { ScreenConnection } from './Screen'

export type DisplayTransport = 'usb' | 'ble' | 'wifi'
export const displayTransportLabels: Record<DisplayTransport, string> = { usb: 'USB', ble: '蓝牙', wifi: 'Wi-Fi' }
export type WifiDisplay = { id: string; name: string; address: string | null }
export type DisplayConnectionStatus = {
  phase: 'waiting' | 'syncing' | 'connected' | 'disconnected' | 'failed'
  transport?: DisplayTransport
  deviceId?: string
  wifiProvisioning?: boolean
  port?: string | null
  brightness?: number
  screenRotated?: boolean | null
  reason?: string | null
  reconnecting?: boolean
  hasLayout?: boolean
  lastReadingsAppliedAtUnixMs?: number | null
  power?: { source: 'external' | 'battery' | 'unknown'; batteryPercent: number | null; charging?: boolean } | null
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
  const power = status?.power
  const screenConnection: ScreenConnection = {
    phase: status?.phase ?? 'waiting',
    transport: status?.transport,
    hasLayout: status?.hasLayout,
    lastReadingsAt: status?.lastReadingsAppliedAtUnixMs == null ? null : status.lastReadingsAppliedAtUnixMs / 1000,
    power: power?.source === 'battery' || power?.charging
      ? { source: 'battery', percent: power.batteryPercent, charging: power.charging }
      : { source: power?.source === 'external' ? 'external' : 'unknown' },
  }
  return {
    status, screenConnection, ports, refreshPorts,
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
    setBrightness: (percent: number) => invoke<void>('set_display_brightness', { percent }),
    setRotation: (rotated: boolean) => invoke<void>('set_display_rotation', { rotated }),
  }
}
