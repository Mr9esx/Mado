import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react'
import { defaultAppearance, latestAppearance, resolveAppTheme, type Appearance } from './appearanceModel'
import type { DisplayTheme, Snapshot } from './layout'

type AppearanceContextValue = {
  appearance: Appearance; ready: boolean; busy: boolean; error: string
  update: (patch: Partial<Pick<Appearance, 'appTheme' | 'displayTheme'>>) => Promise<void>
}
const AppearanceContext = createContext<AppearanceContextValue>({ appearance: defaultAppearance, ready: false, busy: false, error: '', update: async () => {} })
export const useAppearance = () => useContext(AppearanceContext)

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [appearance, setAppearance] = useState(defaultAppearance)
  const [ready, setReady] = useState(!isTauri())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [screenTheme, setScreenTheme] = useState<DisplayTheme>()
  const receive = useCallback((next: Appearance) => setAppearance(current => latestAppearance(current, next)), [])
  useEffect(() => {
    if (!isTauri()) return
    let active = true
    let stop: (() => void) | undefined
    void (async () => {
      try {
        const unlisten = await listen<Appearance>('appearance-changed', ({ payload }) => { if (active) receive(payload) })
        if (!active) { unlisten(); return }
        stop = unlisten
        const saved = await invoke<Appearance>('get_appearance')
        if (active) { receive(saved); setReady(true) }
      } catch (reason) { if (active) setError(`无法读取外观：${String(reason)}`) }
    })()
    return () => { active = false; stop?.() }
  }, [receive])
  useEffect(() => {
    if (!isTauri()) return
    let active = true
    let stop: (() => void) | undefined
    let revision = -1
    const receiveTheme = (snapshot: Snapshot) => {
      if (!active || revision >= snapshot.revision) return
      revision = snapshot.revision
      setScreenTheme(snapshot.theme)
    }
    void (async () => {
      try {
        const unlisten = await listen<Snapshot>('device-snapshot', ({ payload }) => receiveTheme(payload))
        if (!active) { unlisten(); return }
        stop = unlisten
        receiveTheme(await invoke<Snapshot>('get_snapshot'))
      } catch {
        // The normal appearance preference still supplies colors for the preview.
      }
    })()
    return () => { active = false; stop?.() }
  }, [])
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      const theme = resolveAppTheme(appearance.appTheme, media.matches)
      document.documentElement.classList.toggle('dark', theme === 'dark')
      document.documentElement.dataset.appTheme = theme
      document.documentElement.style.colorScheme = theme
      document.querySelector<HTMLLinkElement>('#app-icon')?.setAttribute('href', `/brand/logo-${theme}.svg`)
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [appearance.appTheme])
  useEffect(() => {
    const root = document.documentElement
    root.dataset.displayTheme = screenTheme?.active ?? appearance.displayTheme
    const palette = screenTheme?.[screenTheme.active]
    if (palette) {
      root.style.setProperty('--display-screen', palette.background)
      root.style.setProperty('--display-surface', palette.surface)
      root.style.setProperty('--display-ink', palette.text)
    } else {
      root.style.removeProperty('--display-screen')
      root.style.removeProperty('--display-surface')
      root.style.removeProperty('--display-ink')
    }
  }, [screenTheme, appearance.displayTheme])
  async function update(patch: Partial<Pick<Appearance, 'appTheme' | 'displayTheme'>>) {
    if (busy || !ready) return
    setBusy(true); setError('')
    try {
      if (isTauri()) receive(await invoke<Appearance>('update_appearance', patch))
      else setAppearance(current => ({ ...current, ...patch, revision: current.revision + 1 }))
    } catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }
  return <AppearanceContext.Provider value={{ appearance, ready, busy, error, update }}>{children}</AppearanceContext.Provider>
}
