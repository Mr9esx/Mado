import assert from 'node:assert/strict'
import test from 'node:test'
import { nextPageId, samePages, pageError, pagesFromSnapshot } from '../src/pages.ts'
const card = { id: 'quota', x: 0, y: 0, w: 1, h: 1 }
const pages = [{ id: 'a', name: '首页', layout: [card] }, { id: 'b', name: '系统', layout: [] }]

test('manual and timed navigation wrap in both directions and recover missing current pages', () => {
  assert.equal(nextPageId(pages, 'a', 1), 'b')
  assert.equal(nextPageId(pages, 'b', 1), 'a')
  assert.equal(nextPageId(pages, 'a', -1), 'b')
  assert.equal(nextPageId(pages, 'deleted', 1), 'a')
  assert.equal(nextPageId([], 'a', 1), undefined)
})

test('dirty detection includes page order, names and card positions', () => {
  assert.equal(samePages(pages, structuredClone(pages)), true)
  assert.equal(samePages(pages, [...pages].reverse()), false)
  assert.equal(samePages(pages, [{ ...pages[0], name: '改名' }, pages[1]]), false)
  assert.equal(samePages(pages, [{ ...pages[0], layout: [{ ...card, x: 1 }] }, pages[1]]), false)
})

test('page validation rejects empty names, duplicate IDs and exceeding five pages', () => {
  assert.equal(pageError(pages), '')
  assert.ok(pageError([]))
  assert.ok(pageError([pages[0], pages[0]]))
  assert.ok(pageError([{ ...pages[0], name: '  ' }]))
  assert.ok(pageError(Array.from({ length: 6 }, (_, i) => ({ ...pages[0], id: String(i) }))))
})

test('an older single-page snapshot remains editable without losing its layout', () => {
  const restored = pagesFromSnapshot({ layout: [card] })
  assert.equal(restored.length, 1)
  assert.deepEqual(restored[0].layout, [card])
})
