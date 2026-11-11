import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import type { DisplayPage, DisplayTheme, LayoutItem, Snapshot } from './layout'

export const getSnapshot = () => invoke<Snapshot>('get_snapshot')
export const syncLayout = (layout: LayoutItem[], baseRevision: number) =>
  invoke<Snapshot>('sync_layout', { layout, baseRevision })
export const applyPages = (pages: DisplayPage[], rotationSeconds: number, baseRevision: number, theme?: DisplayTheme) =>
  invoke<Snapshot>('apply_pages', { pages, rotationSeconds, baseRevision, theme })
export const selectPage = (id: string) => invoke<Snapshot>('select_page', { id })
export const openDesktopWidget = () => invoke<void>('open_desktop_widget')

export function useDevice() {
  const [snapshot, setSnapshot] = useState<Snapshot>()
  const [error, setError] = useState('')

  function receive(next: Snapshot) {
    setError('')
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
        if (active) setError(String(reason))
      }
    }

    void connect()
    return () => {
      active = false
      stopListening?.()
    }
  }, [])

  return { snapshot, receive, error }
}
