import geometry from '../shared/display.json' with { type: 'json' }
import codexModule from '../shared/collectors/codex/manifest.json' with { type: 'json' }
import systemModule from '../shared/collectors/system/manifest.json' with { type: 'json' }
import type { PowerStatus } from './status'

export type Cell = { x: number; y: number }
export type Widget = {
  id: string
  group: string
  groupName?: string
  name: string
  shortName: string
  unit: string
  kind: string
  sizes: number[][]
  stateLabels?: Record<string, string>
  accentColor?: string
}
export type LayoutItem = Cell & { id: string; w: number; h: number }
export type DisplayPage = { id: string; name: string; layout: LayoutItem[] }
export type TaskReading = {
  state: 'running' | 'completed' | 'interrupted' | 'error' | 'unknown'
  title: string | null
}
export type Reading = {
  kind: string
  value?: number
  ratio?: number | null
  deadlineUnixMs?: number
  state?: string
  detail?: string | null
  values?: (number | null)[]
  min?: number
  max?: number
  delta?: number | null
  deltaUnit?: string | null
  bounds?: { min: number; max: number }
  notify?: boolean
}
export type WidgetValue = number | TaskReading | Reading
export type Snapshot = {
  pages?: DisplayPage[]
  activePageId?: string
  rotationSeconds?: number
  revision: number
  sequence: number
  layout: LayoutItem[]
  widgets?: Widget[]
  appliedWidgets?: Widget[]
  values: Record<string, WidgetValue>
  lastSyncedAt: number | null
  power: PowerStatus
}

const definitions = [codexModule, systemModule].flatMap(module => Object.entries(module.widgets).map(([key, widget]): Widget => {
  const outputs: Record<string, { valueType: string; unit?: string }> = module.outputs
  return {
    id: module.moduleId === 'codex' ? key : module.moduleId + '.' + key,
    group: module.moduleId,
    groupName: module.name,
    name: widget.label,
    shortName: 'shortLabel' in widget ? widget.shortLabel : widget.label,
    unit: outputs[widget.outputKey].unit ?? '',
    sizes: widget.supportedSpans.map(span => [span.w, span.h]),
    kind: widget.kind,
  }
}))
export const display = { ...geometry, widgets: definitions }
const bundledWidgets = new Map<string, Widget>(display.widgets.map((widget) => [widget.id, widget]))
export const widgetCatalog = (snapshot?: Snapshot) => snapshot?.widgets
  ? new Map(snapshot.widgets.map((widget) => [widget.id, widget])) : bundledWidgets
export const gridWidth = display.width - display.padding * 2
export const gridHeight = display.height - display.padding * 2 - display.gap - display.statusHeight
export const cellWidth = (gridWidth - display.gap * (display.columns - 1)) / display.columns
export const cellHeight = (gridHeight - display.gap * (display.rows - 1)) / display.rows

export function canPlace(layout: LayoutItem[], item: LayoutItem, catalog = bundledWidgets) {
  const widget = catalog.get(item.id)
  if (!widget?.sizes.some(([w, h]) => w === item.w && h === item.h)) return false
  if (!Number.isInteger(item.x) || !Number.isInteger(item.y)) return false
  if (item.x < 0 || item.y < 0 || item.x + item.w > display.columns || item.y + item.h > display.rows) return false

  return !layout.some((other) => other.id !== item.id
    && item.x < other.x + other.w && item.x + item.w > other.x
    && item.y < other.y + other.h && item.y + item.h > other.y)
}

export function firstOpenCell(layout: LayoutItem[], id: string, catalog = bundledWidgets): LayoutItem | undefined {
  for (const [w, h] of catalog.get(id)?.sizes ?? []) {
    for (let y = 0; y < display.rows; y++) {
      for (let x = 0; x < display.columns; x++) {
        const item = { id, x, y, w, h }
        if (canPlace(layout, item, catalog)) return item
      }
    }
  }
}

export function cellAt(rect: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>, clientX: number, clientY: number): Cell | null {
  const x = (clientX - rect.left) / rect.width
  const y = (clientY - rect.top) / rect.height
  if (x < 0 || x >= 1 || y < 0 || y >= 1 || !Number.isFinite(x) || !Number.isFinite(y)) return null

  return {
    x: Math.floor(x * gridWidth / (cellWidth + display.gap)),
    y: Math.floor(y * gridHeight / (cellHeight + display.gap)),
  }
}

export function sameLayout(left: LayoutItem[], right: LayoutItem[]) {
  return left.length === right.length && left.every((item) => right.some((other) =>
    item.id === other.id && item.x === other.x && item.y === other.y && item.w === other.w && item.h === other.h))
}
