import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { isTauri } from '@tauri-apps/api/core'
import { X } from 'lucide-react'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { Editor } from './Editor'
import { Screen } from './Screen'
import { useDevice } from './device'
import './styles.css'

function VirtualDisplay() {
  const { snapshot } = useDevice()
  const displayWindow = getCurrentWebviewWindow()

  return (
    <main className="virtual-window" aria-label="虚拟显示屏" onPointerDown={(event) => {
      if (event.button === 0 && event.target instanceof Element && !event.target.closest('button')) {
        void displayWindow.startDragging().catch(console.error)
      }
    }}>
      <Screen layout={snapshot?.layout ?? []} snapshot={snapshot} />
      <button type="button" className="virtual-window-close" aria-label="关闭虚拟屏" title="关闭虚拟屏"
        onClick={() => void displayWindow.close().catch(console.error)}>
        <X size={14} />
      </button>
    </main>
  )
}

const screenWindow = isTauri() && getCurrentWebviewWindow().label === 'display'
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    {isTauri()
      ? screenWindow ? <VirtualDisplay /> : <Editor />
      : <main className="launch-help"><p>请通过 <code>npm run tauri dev</code> 打开桌面 App。</p></main>}
  </StrictMode>,
)
