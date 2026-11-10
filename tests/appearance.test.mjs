import test from 'node:test'
import assert from 'node:assert/strict'
import { defaultAppearance, resolveAppTheme, latestAppearance } from '../src/appearanceModel.ts'
test('explicit themes override system appearance while system mode follows it', () => {
  assert.equal(resolveAppTheme('light', true), 'light')
  assert.equal(resolveAppTheme('dark', false), 'dark')
  assert.equal(resolveAppTheme('system', true), 'dark')
  assert.equal(resolveAppTheme('system', false), 'light')
  assert.equal(defaultAppearance.displayTheme, 'dark')
})
test('a delayed initial fetch cannot replace a newer cross-window theme change', () => {
  const current = { ...defaultAppearance, appTheme: 'dark', revision: 2 }
  assert.deepEqual(latestAppearance(current, { ...defaultAppearance, revision: 1 }), current)
  const next = { ...current, displayTheme: 'light', revision: 3 }
  assert.deepEqual(latestAppearance(current, next), next)
})
