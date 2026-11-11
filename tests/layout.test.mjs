import assert from 'node:assert/strict'
import test from 'node:test'
import { canPlace, cellAt, cellHeight, cellWidth, display, firstOpenCell, repositionCard } from '../src/layout.ts'

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

test('dragging onto an occupied cell exchanges the two cards and preserves their appearance', () => {
  const colored = { ...quota, appearance: { accentColor: '#397ADC' } }
  assert.deepEqual(repositionCard([colored, speed, resets], { ...colored, x: 2 }), [
    { ...colored, x: 2 }, { ...speed, x: 0 }, resets,
  ])
})

test('a drop can swap with a wider card only when both new footprints fit', () => {
  const wide = { ...quota, w: 2 }
  assert.deepEqual(repositionCard([wide, speed], { ...speed, x: 1 }), [
    { ...wide, x: 2 }, { ...speed, x: 0 },
  ])
  assert.equal(repositionCard([wide, { ...speed, x: 3 }], { ...speed, x: 0 }), undefined)
})

test('a drop cannot displace multiple cards or move another card out of a new card’s way', () => {
  const wide = { ...quota, w: 2 }
  assert.equal(repositionCard([wide, speed, { ...resets, x: 3, y: 0 }], { ...wide, x: 2 }), undefined)
  assert.equal(repositionCard([speed], { ...resets, y: 0 }), undefined)
})

test('scans for a free cell when adding a card', () => {
  assert.deepEqual(firstOpenCell([quota, speed], 'resets'), { ...resets, x: 1, y: 0 })
})

test('resizes only to declared spans that fit the anchored card', () => {
  assert.equal(canPlace([quota], { ...quota, w: 2, h: 2 }), true)
  assert.equal(canPlace([quota, speed], { ...quota, w: 4 }), false)
  assert.equal(canPlace([quota], { ...quota, x: 1, w: 4 }), false)
  assert.equal(canPlace([], { ...quota, w: 1, h: 2 }), false)
  assert.equal(canPlace([], { ...quota, id: 'system.cpu-usage-history', w: 2, h: 2 }), false)
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
