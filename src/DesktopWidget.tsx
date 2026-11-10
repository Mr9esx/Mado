import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { ChevronLeft, ChevronRight, Pause, Play, Pin, PinOff, Settings2, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Screen } from './Screen'
import { WidgetShell } from './WidgetShell'
import { nextPageId, pagesFromSnapshot } from './pages'
import { selectPage, useDevice } from './device'

export function DesktopWidget() {
  const { snapshot, receive, error: dataError } = useDevice()
  const [widgetWindow] = useState(getCurrentWebviewWindow)
  const [pinned, setPinned] = useState<boolean>()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [interacting, setInteracting] = useState(false)
  const [playOverride, setPlayOverride] = useState<boolean | null>(null)
  const changingPage = useRef(false)
  const pages = pagesFromSnapshot(snapshot)
  const activeId = snapshot?.activePageId ?? pages[0].id
  const rotation = snapshot?.rotationSeconds ?? 0
  const playing = (playOverride ?? rotation > 0) && pages.length > 1
  const pageIds = pages.map(page => page.id).join(',')
  async function step(direction: number) {
    const id = nextPageId(pages, activeId, direction)
    if (!id || id === activeId || changingPage.current) return
    changingPage.current = true
    try { receive(await selectPage(id)); setError('') }
    catch (reason) { setError(`无法切换页面：${String(reason)}`); setPlayOverride(false) }
    finally { changingPage.current = false }
  }
  const stepRef = useRef(step)
  stepRef.current = step
  useEffect(() => { setPlayOverride(null) }, [rotation])
  useEffect(() => {
    if (!playing || interacting) return
    const timer = window.setTimeout(() => void stepRef.current(1), (rotation || 10) * 1000)
    return () => window.clearTimeout(timer)
  }, [activeId, pageIds, rotation, playing, interacting])

  useEffect(() => {
    let active = true
    invoke<boolean>('get_desktop_widget_pinned')
      .then(value => { if (active) setPinned(value) })
      .catch(reason => { if (active) setError(`无法读取置顶状态：${String(reason)}`) })
    return () => { active = false }
  }, [])

  async function togglePinned() {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      const current = pinned ?? await invoke<boolean>('get_desktop_widget_pinned')
      setPinned(await invoke<boolean>('set_desktop_widget_pinned', { pinned: !current }))
    } catch (reason) {
      setError(String(reason))
      // Read back the native state if applying or saving the preference failed.
      try { setPinned(await invoke<boolean>('get_desktop_widget_pinned')) } catch { /* keep the error visible */ }
    } finally {
      setBusy(false)
    }
  }

  const PinIcon = pinned ? PinOff : Pin
  const pinLabel = pinned ? '取消置顶' : '置顶桌面控件'
  return (
    <WidgetShell onDrag={() => void widgetWindow.startDragging().catch(reason => setError(`无法移动控件：${String(reason)}`))}
      onPageStep={direction => void step(direction)} onInteractionChange={setInteracting}
      error={error || (dataError ? `无法加载本机数据：${dataError}` : '')} onDismissError={error ? () => setError('') : undefined}
      tools={<>
        {pages.length > 1 && <>
          <button type="button" aria-label="上一页" title="上一页" onClick={() => void step(-1)}><ChevronLeft size={14} /></button>
          <button type="button" aria-label="下一页" title="下一页" onClick={() => void step(1)}><ChevronRight size={14} /></button>
          <button type="button" aria-label={playing ? '暂停轮播' : '开始轮播'} title={playing ? '暂停轮播' : `开始轮播（每 ${rotation || 10} 秒）`}
            aria-pressed={playing} onClick={() => setPlayOverride(!playing)}>{playing ? <Pause size={14} /> : <Play size={14} />}</button>
        </>}
        <button type="button" aria-label="编辑布局" title="编辑布局"
          onClick={() => void invoke('show_editor').catch(reason => setError(`无法打开编辑器：${String(reason)}`))}>
          <Settings2 size={14} />
        </button>
        <button type="button" aria-label={pinLabel} aria-pressed={pinned ?? false} title={pinLabel}
          disabled={busy} onClick={() => void togglePinned()}>
          <PinIcon size={14} />
        </button>
        <button type="button" aria-label="关闭桌面控件" title="关闭桌面控件"
          onClick={() => void widgetWindow.close().catch(reason => setError(`无法关闭控件：${String(reason)}`))}>
          <X size={14} />
        </button>
      </>}>
      <Screen surface="desktop" layout={snapshot?.layout ?? []} pages={pages} pageId={activeId}
        onPageChange={pages.length > 1 ? () => void step(1) : undefined}
        snapshot={snapshot?.appliedWidgets ? { ...snapshot, widgets: snapshot.appliedWidgets } : snapshot} />
    </WidgetShell>
  )
}
