import { FolderOpen, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { Button } from './components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from './components/ui/dialog'
import { Input } from './components/ui/input'
import { CodexSettings } from './CodexSettings'
import { useCodexStatus } from './codex'
import { chooseResource, type Approval, type GrantRequest } from './collectorApi'

export function CollectorAuthorization({ requests, live, onFinish }: {
  requests: GrantRequest[]
  live: boolean
  onFinish: (approval?: Approval) => void
}) {
  const [targets, setTargets] = useState<Record<string, string>>({})
  const [error, setError] = useState('')
  const ready = requests.every(({ key, scope, target }) =>
    target || scope.kind === 'system' || scope.kind === 'browser' || targets[key]?.trim())


  return <Dialog open onOpenChange={open => { if (!open) onFinish() }}>
    <DialogContent className="settings-dialog">
    <form onSubmit={event => {
      event.preventDefault()
      if (ready) onFinish({ requests, targets })
    }}>
      <DialogTitle>确认资源授权</DialogTitle>
      <DialogDescription className="source-caption">{live ? '本次试跑将访问以下资源，同一份草稿发布时可以沿用授权。' : '发布后，应用会按以下范围代脚本访问资源。'}</DialogDescription>
      <div className="collector-grants">
        {requests.map(({ key, label, scope, target }) => {
          const http = scope.kind === 'http' || scope.kind === 'httpJson'
          const summary = scope.kind === 'http' ? (scope.methods ?? ['GET']).join('、') + ' · 同源请求'
            : scope.kind === 'httpJson' ? 'GET · 读取此地址'
            : scope.kind === 'system' ? '读取 CPU、内存和开机时长'
            : scope.kind === 'browser' ? '打开 ' + scope.hosts.join('、') + ' 的 HTTPS 页面'
            : scope.kind === 'process' ? '允许参数 ' + scope.args.map(args => JSON.stringify(args)).join('、')
            : scope.kind === 'stdio' ? '启动参数 ' + JSON.stringify(scope.args) + ' · RPC ' + scope.methods.join('、')
            : scope.kind === 'directory' ? '读取目录及其子目录' : '读取单个文件'
          return <section key={key} className="collector-grant">
            <strong>{label}</strong>
            {target ? <code>{target}</code> : http ? <Input aria-label={label} type="url" required value={targets[key] ?? ''}
              placeholder="https://example.com" onChange={event => setTargets({ ...targets, [key]: event.target.value })} />
              : scope.kind !== 'system' && scope.kind !== 'browser' && <div className="collector-grant-picker">
                <Button type="button" size="sm" variant="outline" onClick={() => {
                  setError('')
                  void chooseResource(scope.kind).then(path => {
                    if (path) setTargets(current => ({ ...current, [key]: path }))
                  }, reason => setError(String(reason)))
                }}><FolderOpen size={14} />{scope.kind === 'directory' ? '选择目录' : scope.kind === 'process' || scope.kind === 'stdio' ? '选择程序' : '选择文件'}</Button>
                {targets[key] && <code>{targets[key]}</code>}
              </div>}
            {summary && <small>{summary}</small>}
          </section>
        })}
      </div>
      {error && <p role="alert" className="collector-error">{error}</p>}
      <footer className="source-sheet-footer">
        <Button type="button" variant="ghost" size="sm" onClick={() => onFinish()}>取消</Button>
        <Button type="submit" size="sm" disabled={!ready}><ShieldCheck size={15} />确认授权</Button>
      </footer>
    </form>
    </DialogContent>
  </Dialog>
}

export function CollectorCodexSettings({ onDismiss }: { onDismiss: () => void }) {
  const { status, error } = useCodexStatus()
  return <CodexSettings status={status} error={error} onDismiss={onDismiss} />
}
