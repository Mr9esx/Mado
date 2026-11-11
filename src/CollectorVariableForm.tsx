import { useId } from 'react'
import { Button } from './components/ui/button'
import { Input } from './components/ui/input'
import type { CollectorVariables } from './collectorApi'

export function collectorVariableChanges(changes: Record<string, string>, key: string, value: string | undefined) {
  const next = { ...changes }
  if (value === undefined) delete next[key]
  else next[key] = value
  return next
}

export function canSaveCollectorVariables({ fields, configured }: CollectorVariables, changes: Record<string, string>) {
  return fields.every(({ key, required }) => !required
    || (Object.hasOwn(changes, key) ? Boolean(changes[key].trim()) : configured[key]))
}

export function CollectorVariableForm({ variables, changes, busy = false, enabled, onChange, onSave, onClose, onToggle }: {
  variables: CollectorVariables
  changes: Record<string, string>
  busy?: boolean
  enabled?: boolean
  onChange: (changes: Record<string, string>) => void
  onSave: () => void
  onClose: () => void
  onToggle?: () => void
}) {
  const formId = useId()
  const ready = canSaveCollectorVariables(variables, changes)
  const edited = Object.keys(changes).length > 0

  return <form className="collector-variable-form" onSubmit={event => {
    event.preventDefault()
    if (ready && edited && !busy) onSave()
  }}>
    {variables.fields.map(({ key, label, description, required, secret, value }) => {
      const id = `${formId}-${key}`
      const changed = Object.hasOwn(changes, key)
      const configured = variables.configured[key]
      const cleared = changed && changes[key] === '' && configured
      return <div key={key} className="collector-variable-field">
        <label htmlFor={id}>{label}{required && <span aria-hidden="true">必填</span>}</label>
        {description && <small id={`${id}-description`}>{description}</small>}
        <div className="collector-variable-input">
          <Input id={id} variant="sm" type={secret ? 'password' : 'text'} autoComplete={secret ? 'new-password' : 'off'}
            aria-required={required} aria-describedby={description ? `${id}-description` : undefined}
            disabled={busy} value={changed ? changes[key] : secret ? '' : value ?? ''}
            onChange={event => {
              const entered = event.target.value
              onChange(collectorVariableChanges(changes, key, secret && entered === '' ? undefined : entered))
            }} />
          {secret && <small>{cleared ? '待清除' : configured ? '已配置' : '未配置'}</small>}
          {!required && (configured || changed) && <Button type="button" size="xs" variant="ghost" disabled={busy}
            aria-label={(cleared ? '撤销清除' : '清除') + label}
            onClick={() => onChange(collectorVariableChanges(changes, key, cleared ? undefined : ''))}>
            {cleared ? '撤销清除' : '清除'}
          </Button>}
        </div>
      </div>
    })}
    <div className="collector-form-actions">
      {onToggle && <Button size="sm" variant="outline" disabled={busy || edited || (!enabled && !ready)} onClick={onToggle}>
        {enabled ? '停用采集' : '启用采集'}
      </Button>}
      <Button size="sm" variant="ghost" disabled={busy} onClick={onClose}>关闭</Button>
      <Button size="sm" type="submit" disabled={busy || !ready || !edited}>保存变量</Button>
    </div>
  </form>
}
