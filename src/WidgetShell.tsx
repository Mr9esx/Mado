import { useEffect, useRef, useState, type ReactNode } from 'react'
import { X } from 'lucide-react'

type Props = {
  children: ReactNode
  tools: ReactNode
  error?: string
  onDismissError?: () => void
  onDrag?: () => void
  onPageStep?: (direction: number) => void
  onInteractionChange?: (active: boolean) => void
}

export function WidgetShell({ children, tools, error, onDismissError, onDrag, onPageStep, onInteractionChange }: Props) {
  const root = useRef<HTMLElement>(null)
  const toolbar = useRef<HTMLDivElement>(null)
  const [hovered, setHovered] = useState(false)
  const [keyboardFocused, setKeyboardFocused] = useState(false)
  const visible = hovered || keyboardFocused

  function hideControls() {
    setHovered(false)
    setKeyboardFocused(false)
    if (toolbar.current?.contains(document.activeElement) && document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
  }
  useEffect(() => {
    window.addEventListener('blur', hideControls)
    return () => window.removeEventListener('blur', hideControls)
  }, [])
  useEffect(() => { onInteractionChange?.(visible) }, [visible, onInteractionChange])

  return <main ref={root} tabIndex={0} className="desktop-widget" aria-label="桌面控件" data-controls-visible={visible}
    onPointerEnter={() => setHovered(true)} onPointerMove={() => setHovered(true)} onPointerLeave={hideControls}
    onFocusCapture={event => setKeyboardFocused(event.target instanceof Element && event.target.matches(':focus-visible'))}
    onBlurCapture={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setKeyboardFocused(false) }}
    onPointerDown={event => {
      if (event.button === 0 && event.target instanceof Element && !event.target.closest('button, [role="alert"]')) {
        root.current?.focus({ preventScroll: true })
        onDrag?.()
      }
    }}
    onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); hideControls() }
      if (onPageStep && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        event.preventDefault()
        onPageStep(event.key === 'ArrowLeft' ? -1 : 1)
      }
    }}>
    {children}
    <div ref={toolbar} className="desktop-widget-tools" role="toolbar" aria-label="桌面控件操作">{tools}</div>
    {error && <div className="desktop-widget-error" role="alert"><span>{error}</span>
      {onDismissError && <button type="button" aria-label="关闭错误提示" onClick={onDismissError}><X size={12} /></button>}
    </div>}
  </main>
}
