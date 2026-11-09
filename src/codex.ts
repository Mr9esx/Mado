import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

type Phase = 'disabled' | 'checking' | 'signingIn' | 'connected' | 'needsCli' | 'needsLogin' | 'unsupportedLogin' | 'error'

export type CodexStatus = {
  sequence: number
  enabled: boolean
  cliPath: string | null
  resolvedCli: string | null
  phase: Phase
  message: string | null
  plan: string | null
  limits: {
    windows: { durationMinutes: number | null; remainingPercent: number; resetsAt: number | null }[]
    resetCount: number | null
  }
  accountUpdatedAt: number | null
  speed: { tokensPerSecond: number; samples: number; latestAt: number } | null
}

const labels: Record<Phase, string> = {
  disabled: '未连接',
  checking: '检测中',
  signingIn: '登录中',
  connected: '已连接',
  needsCli: '缺少 CLI',
  needsLogin: '待登录',
  unsupportedLogin: '需 ChatGPT 登录',
  error: '连接异常',
}

export const codexLabel = (status?: CodexStatus) => status ? labels[status.phase] : '读取中'
export const connectCodex = (cliPath: string) => invoke<void>('connect_codex', { cliPath })
export const refreshCodex = () => invoke<void>('refresh_codex')
export const pauseCodex = () => invoke<void>('pause_codex')
export const loginCodex = () => invoke<void>('login_codex')
export const cancelCodexLogin = () => invoke<void>('cancel_codex_login')

export function useCodexStatus() {
  const [status, setStatus] = useState<CodexStatus>()
  const [error, setError] = useState('')

  useEffect(() => {
    let alive = true
    let unlisten: (() => void) | undefined

    function receive(next: CodexStatus) {
      if (alive) setStatus((previous) => previous && previous.sequence > next.sequence ? previous : next)
    }

    async function connect() {
      try {
        const stop = await listen<CodexStatus>('codex-status', ({ payload }) => receive(payload))
        if (!alive) {
          stop()
          return
        }
        unlisten = stop
        receive(await invoke<CodexStatus>('get_codex_status'))
      } catch (reason) {
        if (alive) setError(String(reason))
      }
    }

    void connect()
    return () => {
      alive = false
      unlisten?.()
    }
  }, [])

  return { status, error }
}
