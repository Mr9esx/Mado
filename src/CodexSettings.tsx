import { X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cancelCodexLogin, codexLabel, connectCodex, loginCodex, pauseCodex, refreshCodex, type CodexStatus } from './codex'

const resetTime = new Intl.DateTimeFormat('zh-CN', {
  month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
})
const updateTime = new Intl.DateTimeFormat('zh-CN', {
  hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
})

function windowName(minutes: number | null) {
  if (!minutes) return '剩余额度'
  if (minutes % 1440 === 0) return `${minutes / 1440} 天剩余`
  if (minutes % 60 === 0) return `${minutes / 60} 小时剩余`
  return `${minutes} 分钟剩余`
}

function resetLabel(seconds: number | null) {
  if (seconds == null) return null
  const date = new Date(seconds * 1000)
  return Number.isNaN(date.getTime()) ? null : `${resetTime.format(date)} 重置`
}

export function CodexSettings({ status, error, onDismiss }: {
  status?: CodexStatus
  error: string
  onDismiss: () => void
}) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [pathDraft, setPath] = useState<string>()
  const [pending, setPending] = useState(false)
  const [actionError, setActionError] = useState('')
  const signingIn = status?.phase === 'signingIn'
  const checking = pending || status?.phase === 'checking' || signingIn
  const path = pathDraft ?? status?.cliPath ?? ''
  const pathChanged = path.trim() !== (status?.cliPath ?? '')
  const connected = status?.phase === 'connected'
  const canRefresh = connected && !pathChanged
  const needsLogin = !pathChanged && (status?.phase === 'needsLogin' || status?.phase === 'unsupportedLogin')
  const missingCli = !pathChanged && (status?.phase === 'needsCli' || (status?.phase === 'disabled' && !status.resolvedCli))
  const notice = actionError || error || (missingCli
    ? status?.cliPath ? '此路径下找不到 Codex CLI。' : '未找到 Codex CLI。'
    : status?.phase === 'needsCli' ? null : status?.message)

  useEffect(() => {
    if (dialog.current && !dialog.current.open) dialog.current.showModal()
  }, [])

  async function run(action: () => Promise<void>) {
    setPending(true)
    setActionError('')
    try {
      await action()
    } catch (reason) {
      setActionError(String(reason))
    } finally {
      setPending(false)
    }
  }

  return (
    <dialog ref={dialog} className="source-dialog" aria-labelledby="source-settings-title"
      onClose={onDismiss} onClick={(event) => { if (event.target === event.currentTarget) onDismiss() }}>
      <div className="source-sheet">
        <header className="source-sheet-header">
          <h2 id="source-settings-title">数据源</h2>
          <Button size="sm" mode="icon" variant="ghost" aria-label="关闭设置" onClick={onDismiss}><X className="size-5" /></Button>
        </header>

        <section className="source-connect" aria-labelledby="codex-source-title">
          <div className="source-heading">
            <h3 id="codex-source-title">Codex</h3>
            <span className={`source-chip${connected ? ' is-connected' : ''}`}>
              <i aria-hidden="true" />{codexLabel(status)}
            </span>
          </div>
          <p className="source-notice" role="status">{notice}</p>
          {!notice && status?.phase === 'disabled' && <p className="source-caption">沿用 Codex CLI 的 ChatGPT 登录。</p>}
          {missingCli && !status?.cliPath && (
            <p className="source-caption">终端安装命令 <code>npm install -g @openai/codex</code></p>
          )}
          <details className="source-path">
            <summary>CLI 路径</summary>
            <Input aria-label="Codex CLI 可执行文件路径" value={path} onChange={(event) => setPath(event.target.value)}
              placeholder="留空自动查找" disabled={checking} autoComplete="off" spellCheck={false} />
            {!pathChanged && !path && status?.resolvedCli && <p className="source-caption"><code>{status.resolvedCli}</code></p>}
          </details>
          {needsLogin && <p className="source-caption">也可在终端运行 <code>codex login</code>，再点「重新检测」。</p>}
        </section>

        {(connected || status?.speed) && <section className="source-metrics" aria-labelledby="source-metrics-title">
          <div className="source-heading">
            <h3 id="source-metrics-title">采集结果</h3>
            {status?.accountUpdatedAt != null && <span className="source-caption">额度更新于 {updateTime.format(status.accountUpdatedAt * 1000)}</span>}
          </div>
          <dl className="source-data">
            {status?.limits.windows.map((window, index) => {
              const reset = resetLabel(window.resetsAt)
              return (
                <div key={index}>
                  <dt>{windowName(window.durationMinutes)}</dt>
                  <dd><strong>{window.remainingPercent}%</strong>{reset && <span>{reset}</span>}</dd>
                </div>
              )
            })}
            {connected && !status.limits.windows.length && <div><dt>剩余额度</dt><dd>—</dd></div>}
            {connected && <div>
              <dt>重置卡</dt>
              <dd>{status.limits.resetCount == null ? '—' : <strong>{status.limits.resetCount} 张</strong>}</dd>
            </div>}
            <div>
              <dt>响应速度</dt>
              <dd>{status?.speed
                ? <><strong>{status.speed.tokensPerSecond} tok/s</strong><span>近 {status.speed.samples} 次 · 估算</span></>
                : <span>暂无样本</span>}</dd>
            </div>
          </dl>
          {(status?.limits.windows.length ?? 0) > 1 && <p className="source-caption">屏幕显示剩余比例最低的额度。</p>}
        </section>}

        <footer className="source-sheet-footer">
          {status?.enabled && (
            <Button size="sm" variant="ghost" className="action-button" disabled={pending}
              onClick={() => void run(signingIn ? cancelCodexLogin : pauseCodex)}>
              {signingIn ? '取消登录' : '暂停采集'}
            </Button>
          )}
          {needsLogin && (
            <Button size="sm" variant="ghost" className="action-button" disabled={checking}
              onClick={() => void run(() => connectCodex(path))}>重新检测</Button>
          )}
          <Button size="sm" className="action-button" disabled={checking || !status}
            onClick={() => void run(canRefresh ? refreshCodex : needsLogin ? loginCodex : () => connectCodex(path))}>
            {signingIn ? '等待登录' : checking ? '检测中…' : canRefresh ? '刷新' : needsLogin ? '浏览器登录' : '连接 Codex'}
          </Button>
        </footer>
      </div>
    </dialog>
  )
}
