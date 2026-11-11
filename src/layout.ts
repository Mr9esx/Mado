import geometry from '../shared/display.json' with { type: 'json' }
import codexModule from '../shared/collectors/codex/manifest.json' with { type: 'json' }
import systemModule from '../shared/collectors/system/manifest.json' with { type: 'json' }
import type { PowerStatus } from './status'

export type Cell = { x: number; y: number }
export type NumberAnimation = 'none' | 'digitRoll' | 'count'
export type StateStyle = { color?: string; alertText?: string; alertDurationMs?: number }
export type CardAppearance = {
  accentColor?: string
  numberAnimation?: NumberAnimation
  stateStyles?: Record<string, StateStyle>
}
export type Palette = { background: string; surface: string; text: string }
export type ThemeName = 'dark' | 'light'
export type DisplayTheme = { active: ThemeName; dark: Palette; light: Palette }
export const defaultDisplayTheme: DisplayTheme = {
  active: 'dark',
  dark: { background: '#1E1E1E', surface: '#4A4A4A', text: '#F1F1F1' },
  light: { background: '#E3E3E3', surface: '#FAFAFA', text: '#242424' },
}

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
  supportsNotify?: boolean
  numberAnimation?: NumberAnimation
  stateStyles?: Record<string, StateStyle>
  detailFields?: { key: string; label: string }[]
  format?: string
  expiredText?: string
  chart?: { series: 'bar'; rangeSeconds: number; sampleCount: number; min: number; max: number }
  trend?: { comparisonSeconds: number; deltaUnit?: string | null }
  range?: { mode: 'fixed' | 'reading'; min?: number; max?: number }
}
export type LayoutItem = Cell & { id: string; w: number; h: number; appearance?: CardAppearance }
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
  details?: Record<string, string>
}
export type WidgetValue = number | TaskReading | Reading
export type LocalDisplayFrame = {
  config: {
    theme: DisplayTheme
    pages: { pageId: string; cards: { cardId: string; render: Partial<Widget> & { label: string } }[] }[]
  }
  readings: Record<string, Reading | null>
  sourceIds: Record<string, string>
  configRevision: number
  sequence: number
}
export type Snapshot = {
  displayFrame?: LocalDisplayFrame
  pages?: DisplayPage[]
  activePageId?: string
  rotationSeconds?: number
  revision: number
  sequence: number
  layout: LayoutItem[]
  widgets?: Widget[]
  appliedWidgets?: Widget[]
  values: Record<string, WidgetValue | null>
  lastSyncedAt: number | null
  lastReadingsAt?: number | null
  theme?: DisplayTheme
  power: PowerStatus
}

export type WidgetDefinition = Pick<Widget, 'kind' | 'format' | 'accentColor' | 'stateLabels' | 'stateStyles'
  | 'supportsNotify' | 'numberAnimation' | 'chart' | 'trend' | 'range' | 'detailFields' | 'expiredText'> & {
  outputKey: string; label: string; shortLabel?: string; supportedSpans: { w: number; h: number }[]
}

const definitions = [codexModule, systemModule].flatMap(module => Object.entries(module.widgets).map(([key, definition]): Widget => {
  const { outputKey, label, shortLabel, supportedSpans, ...render } = definition as WidgetDefinition
  const outputs: Record<string, { valueType: string; unit?: string }> = module.outputs
  return {
    ...render,
    id: module.moduleId === 'codex' ? key : module.moduleId + '.' + key,
    group: module.moduleId,
    groupName: module.name,
    name: label,
    shortName: shortLabel ?? label,
    unit: outputs[outputKey].unit ?? '',
    sizes: supportedSpans.map(span => [span.w, span.h]),
  }
}))
export const display = { ...geometry, widgets: definitions }
const bundledWidgets = new Map<string, Widget>(display.widgets.map((widget) => [widget.id, widget]))
export const widgetCatalog = (snapshot?: Snapshot) => snapshot?.widgets
  ? new Map(snapshot.widgets.map((widget) => [widget.id, widget])) : bundledWidgets
export const gridWidth = display.width - display.padding * 2
export const gridHeight = display.height - 2 * display.padding - display.gap - display.statusHeight
export const cellWidth = (gridWidth - display.gap * (display.columns - 1)) / display.columns
export const cellHeight = (gridHeight - display.gap * (display.rows - 1)) / display.rows

export function canPlace(layout: LayoutItem[], item: LayoutItem, catalog = bundledWidgets) {
  const widget = catalog.get(item.id)
  if (!widget?.sizes.some(([w, h]) => w === item.w && h === item.h)) return false
  if (!Number.isInteger(item.x) || !Number.isInteger(item.y)) return false
  if (item.x < 0 || item.y < 0 || item.x + item.w > display.columns || item.y + item.h > display.rows) return false

  return !layout.some(other => other.id !== item.id && overlaps(item, other))
}

function overlaps(item: LayoutItem, other: LayoutItem) {
  return item.x < other.x + other.w && item.x + item.w > other.x
    && item.y < other.y + other.h && item.y + item.h > other.y
}

export function repositionCard(layout: LayoutItem[], item: LayoutItem, catalog = bundledWidgets): LayoutItem[] | undefined {
  const source = layout.find(card => card.id === item.id)
  if (canPlace(layout, item, catalog)) {
    return source ? layout.map(card => card.id === item.id ? item : card) : [...layout, item]
  }
  if (!source) return

  const collisions = layout.filter(card => card.id !== item.id && overlaps(item, card))
  if (collisions.length !== 1) return
  const [other] = collisions
  const moved = { ...item, x: other.x, y: other.y }
  const swapped = { ...other, x: source.x, y: source.y }
  const next = layout.map(card => card.id === moved.id ? moved : card.id === swapped.id ? swapped : card)
  if (canPlace(next, moved, catalog) && canPlace(next, swapped, catalog)) return next
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
    item.id === other.id && item.x === other.x && item.y === other.y && item.w === other.w && item.h === other.h
    && sameAppearance(item.appearance, other.appearance)))
}

function sameAppearance(a?: CardAppearance, b?: CardAppearance) {
  if (a?.accentColor !== b?.accentColor || a?.numberAnimation !== b?.numberAnimation) return false
  const left = a?.stateStyles ?? {}
  const right = b?.stateStyles ?? {}
  return Object.keys(left).length === Object.keys(right).length
    && Object.entries(left).every(([state, style]) => {
      const other = right[state]
      return other && style.color === other.color && style.alertText === other.alertText
        && style.alertDurationMs === other.alertDurationMs
    })
}

export function cardWidget(widget: Widget, appearance?: CardAppearance): Widget {
  if (!appearance) return widget
  const stateStyles = { ...widget.stateStyles }
  for (const [state, style] of Object.entries(appearance.stateStyles ?? {})) {
    stateStyles[state] = { ...stateStyles[state], ...style }
  }
  return {
    ...widget,
    accentColor: appearance.accentColor ?? widget.accentColor,
    numberAnimation: appearance.numberAnimation ?? widget.numberAnimation,
    stateStyles,
  }
}
