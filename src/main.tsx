import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { isTauri } from '@tauri-apps/api/core'
import { lazy, StrictMode, Suspense } from 'react'
import { createRoot } from 'react-dom/client'
import { AppearanceProvider } from './appearance'
import { Editor } from './Editor'
import { DesktopWidget } from './DesktopWidget'
import './styles.css'

const CollectorWorkspace = lazy(() => import('./CollectorWorkspace').then(module => ({ default: module.CollectorWorkspace })))
const windowLabel = isTauri() ? getCurrentWebviewWindow().label : undefined
createRoot(document.getElementById('root')!).render(
  <StrictMode><AppearanceProvider>
    {isTauri()
      ? windowLabel === 'display' ? <DesktopWidget /> : windowLabel === 'collectors'
        ? <Suspense fallback={<main className="studio">
          <div className="window-grip" data-tauri-drag-region aria-hidden="true" />
          <p className="launch-help" role="status">正在打开采集器工作区…</p>
        </main>}><CollectorWorkspace /></Suspense>
        : <Editor />
      : <main className="launch-help"><p>请通过 <code>npm run tauri dev</code> 打开桌面 App。</p></main>}
  </AppearanceProvider></StrictMode>,
)
