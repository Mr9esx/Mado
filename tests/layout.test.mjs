import assert from 'node:assert/strict'
import test from 'node:test'
import { canPlace, cellAt, cellHeight, cellWidth, display, firstOpenCell } from '../src/layout.ts'

const quota = { id: 'quota', x: 0, y: 0, w: 1, h: 1 }
const speed = { id: 'speed', x: 2, y: 0, w: 1, h: 1 }
const resets = { id: 'resets', x: 2, y: 1, w: 1, h: 1 }

test('moves an existing card into a free cell without colliding with itself', () => {
  const layout = [quota, speed, resets]
  assert.equal(canPlace(layout, { ...resets, x: 3 }), true)
  assert.equal(canPlace(layout, resets), true)
  assert.equal(canPlace(layout, { ...resets, x: quota.x, y: quota.y }), false)
})

test('rejects grid overflow and unsupported spans', () => {
  assert.equal(canPlace([], { ...speed, x: display.columns }), false)
  assert.equal(canPlace([], { ...speed, w: 1, h: 2 }), false)
  assert.equal(canPlace([], { ...resets, x: -1 }), false)
})

test('scans for a free cell when adding a card', () => {
  assert.deepEqual(firstOpenCell([quota, speed], 'resets'), { ...resets, x: 1, y: 0 })
})

test('every component only supports one grid cell', () => {
  for (const { id } of display.widgets) {
    assert.equal(canPlace([], { id, x: display.columns - 1, y: display.rows - 1, w: 1, h: 1 }), true, id)
    for (const [w, h] of [[1, 2], [2, 1], [2, 2]]) {
      assert.equal(canPlace([], { id, x: 0, y: 0, w, h }), false, `${id} ${w} × ${h}`)
    }
  }
})

test('the initial layout uses valid one-cell placements', () => {
  for (const item of display.initialLayout) {
    assert.deepEqual([item.w, item.h], [1, 1], item.id)
    assert.equal(canPlace(display.initialLayout, item), true, item.id)
  }
})

test('maps a scaled grid to cells and excludes the status bar', () => {
  const rect = { left: 20, top: 30, width: 624, height: 264 }
  assert.deepEqual(cellAt(rect, 520, 175), { x: 3, y: 1 })
  assert.equal(cellAt(rect, 520, 301), null)
})

test('uses the device cell dimensions for thumbnails', () => {
  assert.deepEqual([cellWidth, cellHeight], [75, 64])
})
