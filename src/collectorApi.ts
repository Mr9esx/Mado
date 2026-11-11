import { invoke } from '@tauri-apps/api/core'
import type { WidgetDefinition } from './layout'

export type Draft = {
  id: string
  definition: string
  files: Record<string, string>
  directories: string[]
  fixture: string
  entry: string
  sdkApiVersion: number
}

export type CollectorInstance = {
  id: string
  name: string
  enabled: boolean
  updatedAt?: number | null
  error?: string | null
  activeAction?: string | null
}

export type InstanceChange = { kind: 'create' } | { kind: 'rename'; id: string; name: string } | { kind: 'delete'; id: string }

export type ModuleSummary = {
  id: string
  name: string
  builtIn: boolean
  revision: number | null
  canRestore: boolean
  changed: boolean
  actions: string[]
  actionInstance: string | null
  activeAction: string | null
  allowsMultipleInstances: boolean
  variableInstances: CollectorInstance[]
}

export type Catalog = {
  modules: ModuleSummary[]
  status: Record<string, { updatedAt?: number; error?: string }>
}

export type CollectorVariables = {
  fields: { key: string; label: string; description?: string | null; required: boolean; secret: boolean; value?: string }[]
  configured: Record<string, boolean>
}

type CollectorInstanceStatus = { enabled: boolean; error: string | null; sequence: number }

export type TestResult = {
  result: { outputs: Record<string, Record<string, unknown> | null>; notifications: string[] } | null
  error: string | null
  logs: string[]
  definition?: Definition
}

export type ImportReview = { draft: Draft; conflict: boolean; identical: boolean }

export type Permission = { label: string } & (
  | { kind: 'http'; methods?: string[] }
  | { kind: 'file' | 'directory' | 'system' | 'notification' }
  | { kind: 'process'; args: string[][] }
  | { kind: 'stdio'; args: string[]; methods: string[] }
  | { kind: 'browser'; hosts: string[] }
)

export type GrantRequest = {
  key: string
  label: string
  scope: Permission | { kind: 'httpJson' | 'fileJson'; label: string }
  target: string | null
}

export type Approval = {
  requests: GrantRequest[]
  targets: Record<string, string>
}

export type Definition = {
  name?: string
  inputs?: Record<string, { kind: 'httpJson' | 'fileJson' | 'native'; label: string }>
  settings?: Record<string, { label: string; default?: string; description?: string; required?: boolean; secret?: boolean }>
  permissions?: Record<string, Permission>
  schedule?: { refreshSeconds: number; enabled: boolean }
  instances?: { mode: 'single' | 'multiple'; batch?: boolean }
  resources?: Record<string, string>
  outputs?: Record<string, { valueType: string; unit?: string }>
  widgets?: Record<string, WidgetDefinition>
}

function labeledFields<T extends { label: string }>(fields?: Record<string, T>) {
  return Object.fromEntries(Object.entries(fields ?? {}).filter(([, field]) =>
    field && typeof field === 'object' && typeof field.label === 'string'))
}

export function readDefinition(text: string): Definition {
  try {
    const definition: unknown = JSON.parse(text)
    if (!definition || typeof definition !== 'object' || Array.isArray(definition)) return {}
    const record = definition as Definition
    return {
      ...record,
      name: typeof record.name === 'string' ? record.name : undefined,
      inputs: labeledFields(record.inputs),
      settings: labeledFields(record.settings),
      permissions: labeledFields(record.permissions),
    }
  } catch {
    return {}
  }
}

export const openCollectors = () => invoke<void>('open_collectors')
export const focusLayout = () => invoke<void>('focus_layout')
export const getCatalog = () => invoke<Catalog>('collector_catalog')
export const getCollectorVariables = (id: string) => invoke<CollectorVariables>('get_collector_variables', { id })
export const saveCollectorVariables = (id: string, values: Record<string, string>) =>
  invoke<CollectorInstanceStatus>('save_collector_variables', { id, values })
export const setCollectorEnabled = (id: string, enabled: boolean, approval?: Approval) =>
  invoke<CollectorInstanceStatus>('set_collector_enabled', { id, enabled, approval })
export const manageCollectorInstance = (moduleId: string, change: InstanceChange) =>
  invoke<string>('manage_collector_instance', { moduleId, change })
export const getInstanceGrants = (id: string) => invoke<GrantRequest[]>('collector_instance_grants', { id })
export const runCollectorAction = (id: string, action: string) =>
  invoke<void>('run_collector_action', { id, action })
export const cancelCollectorAction = (id: string) =>
  invoke<void>('cancel_collector_action', { id })
export const getDraft = (id: string) => invoke<Draft>('collector_draft', { id })
export const createDraft = (name: string, template: string) => invoke<Draft>('create_collector', { name, template })
export const saveDraft = (draft: Draft) => invoke<void>('save_collector', { draft })
export const getGrantRequests = (target: Draft, restore: boolean, live: boolean, instanceId?: string) =>
  invoke<GrantRequest[]>('collector_grants', { target, restore, live, instanceId })
export const testDraft = (target: Draft, live: boolean, approval?: Approval, instanceId?: string) =>
  invoke<TestResult>('test_collector', { target, live, approval, instanceId })
export const chooseResource = (kind: string) => invoke<string | null>('choose_json_input', { kind })
export const publishDraft = (id: string, restore: boolean, approval?: Approval) => invoke<Catalog>('publish_collector', { id, restore, approval })
export const inspectImport = () => invoke<ImportReview | null>('inspect_collector_import')
export const importModule = (draft: Draft, mode: 'new' | 'update' | 'copy') => invoke<Draft>('import_collector', { draft, mode })
export const exportModule = (id: string) => invoke<string | null>('export_collector', { id })
