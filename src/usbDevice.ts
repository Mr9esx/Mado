import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { Snapshot } from './layout'

type UsbPower = { source: 'external' | 'battery' | 'unknown'; batteryPercent: number | null; charging?: boolean }
export type UsbDeviceStatus = {
  phase: 'waiting' | 'syncing' | 'connected' | 'disconnected' | 'failed'
  transport?: 'usb'
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
  power?: UsbPower
}

export function useUsbDevice() {
  const [status, setStatus] = useState<UsbDeviceStatus>()
  const [ports, setPorts] = useState<string[]>([])
  const refreshPorts = async () => setPorts(await invoke<string[]>('list_usb_displays'))
  useEffect(() => {
    let mounted = true
    let eventSeen = false
    const events = listen<UsbDeviceStatus>('usb-display-status', ({ payload }) => {
      eventSeen = true
      if (mounted) setStatus(payload)
    })
    void events.then(async () => {
      const value = await invoke<UsbDeviceStatus>('get_usb_display_status')
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
    connect: (port: string, intervalMs = 1000) => invoke<void>('connect_usb_display', {
      port, intervalMs, utcOffsetMinutes: -new Date().getTimezoneOffset(),
    }),
    disconnect: () => invoke<void>('disconnect_usb_display'),
    selectPage: (pageId: string) => invoke<void>('select_usb_display_page', { pageId }),
    setBrightness: (percent: number) => invoke<void>('set_usb_display_brightness', { percent }),
    setRotation: (rotated: boolean) => invoke<void>('set_usb_display_rotation', { rotated }),
  }
}
