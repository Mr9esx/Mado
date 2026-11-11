import assert from 'node:assert/strict'
import test from 'node:test'
import { defaultDisplayTheme } from '../src/layout.ts'
import { recordLayoutEdit } from '../src/layoutDraft.ts'

const first = { id: 'codex', name: 'Codex', layout: [] }
const second = { id: 'system', name: '系统', layout: [{ id: 'cpu', x: 0, y: 0, w: 1, h: 1 }] }
const saved = { pages: [first, second], rotationSeconds: 0, theme: defaultDisplayTheme, baseRevision: 7 }

test('undo restores a deleted page and its cards without losing the earlier rename', () => {
  const renamed = { ...first, name: '工作台' }
  const rename = recordLayoutEdit(null, saved, { ...saved, pages: [renamed, second] }, first.id)
  const deletion = recordLayoutEdit(rename, saved, { ...saved, pages: [renamed] }, second.id)

  assert.deepEqual(deletion.draft.pages, [renamed])
  assert.deepEqual(deletion.previous.draft.pages, [renamed, second])
  assert.equal(deletion.pageBeforeEdit, second.id)
  assert.equal(deletion.previous.previous, null)
})

test('later display changes undo one at a time and keep the original save revision', () => {
  const withRotation = recordLayoutEdit(null, saved, { ...saved, rotationSeconds: 10 }, first.id)
  const nextTheme = { ...defaultDisplayTheme, active: 'light' }
  const withTheme = recordLayoutEdit(withRotation, { ...saved, baseRevision: 8 }, {
    ...withRotation.draft, theme: nextTheme,
  }, first.id)

  assert.equal(withTheme.draft.theme.active, 'light')
  assert.equal(withTheme.draft.baseRevision, 7)
  assert.equal(withTheme.previous.draft.rotationSeconds, 10)
  assert.deepEqual(withTheme.previous.draft.theme, defaultDisplayTheme)
  assert.equal(withTheme.previous.previous, null)
})

test('reselecting an unchanged option does not add an undo step', () => {
  assert.equal(recordLayoutEdit(null, saved, structuredClone(saved), first.id), null)
  const history = recordLayoutEdit(null, saved, { ...saved, rotationSeconds: 5 }, first.id)
  assert.equal(recordLayoutEdit(history, saved, structuredClone(history.draft), second.id), history)
})
