import display from '../shared/display.json' with { type: 'json' }
import type { PowerStatus } from './status'

export type Cell = { x: number; y: number }
export type Widget = (typeof display.widgets)[number]
export type LayoutItem = Cell & { id: string; w: number; h: number }
export type TaskReading = {
  state: 'running' | 'completed' | 'interrupted' | 'error' | 'unknown'
  title: string | null
}
export type WidgetValue = number | TaskReading
export type Snapshot = {
  revision: number
  sequence: number
  layout: LayoutItem[]
  values: Record<string, WidgetValue>
  lastSyncedAt: number | null
  power: PowerStatus
}

export { display }
export const widgets = new Map(display.widgets.map((widget) => [widget.id, widget]))
export const gridWidth = display.width - display.padding * 2
export const gridHeight = display.height - display.padding * 2 - display.gap - display.statusHeight
export const cellWidth = (gridWidth - display.gap * (display.columns - 1)) / display.columns
export const cellHeight = (gridHeight - display.gap * (display.rows - 1)) / display.rows

export function canPlace(layout: LayoutItem[], item: LayoutItem) {
  const widget = widgets.get(item.id)
  if (!widget?.sizes.some(([w, h]) => w === item.w && h === item.h)) return false
  if (!Number.isInteger(item.x) || !Number.isInteger(item.y)) return false
  if (item.x < 0 || item.y < 0 || item.x + item.w > display.columns || item.y + item.h > display.rows) return false

  return !layout.some((other) => other.id !== item.id
    && item.x < other.x + other.w && item.x + item.w > other.x
    && item.y < other.y + other.h && item.y + item.h > other.y)
}

export function firstOpenCell(layout: LayoutItem[], id: string): LayoutItem | undefined {
  for (const [w, h] of widgets.get(id)?.sizes ?? []) {
    for (let y = 0; y < display.rows; y++) {
      for (let x = 0; x < display.columns; x++) {
        const item = { id, x, y, w, h }
        if (canPlace(layout, item)) return item
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
