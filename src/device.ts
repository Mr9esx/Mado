import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { LayoutItem, Snapshot } from './layout'

export const getSnapshot = () => invoke<Snapshot>('get_snapshot')
export const syncLayout = (layout: LayoutItem[], baseRevision: number) =>
  invoke<Snapshot>('sync_layout', { layout, baseRevision })
export const openDisplay = () => invoke<void>('open_display')

export function useDevice() {
  const [snapshot, setSnapshot] = useState<Snapshot>()

  function receive(next: Snapshot) {
    setSnapshot((current) => current && current.sequence > next.sequence ? current : next)
  }

  useEffect(() => {
    let active = true
    let stopListening: (() => void) | undefined

    async function connect() {
      try {
        const stop = await listen<Snapshot>('device-snapshot', ({ payload }) => {
          if (active) receive(payload)
        })
        if (!active) {
          stop()
          return
        }
        stopListening = stop
        const initial = await getSnapshot()
        if (active) receive(initial)
      } catch (reason) {
        if (active) console.error(reason)
      }
    }

    void connect()
    return () => {
      active = false
      stopListening?.()
    }
  }, [])

  return { snapshot, receive }
}
