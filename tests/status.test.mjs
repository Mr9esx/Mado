import assert from 'node:assert/strict'
import test from 'node:test'
import { formatCountdown, formatSyncTime } from '../src/status.ts'

test('shows a zero-padded sync timestamp with colon-separated date and time', () => {
  const seconds = new Date(2026, 0, 9, 7, 5, 3).getTime() / 1000
  assert.equal(formatSyncTime(seconds), '2026:01:09 07:05:03')
})

test('does not report a sync time before a layout has synced', () => {
  assert.equal(formatSyncTime(null), '—')
})

test('shows compact reset durations and distinguishes expired and missing timestamps', () => {
  assert.equal(formatCountdown(undefined, 100), '—')
  assert.equal(formatCountdown(100, 100), '待刷新')
  assert.equal(formatCountdown(109, 100), '9s')
  assert.equal(formatCountdown(1000, 100), '15m')
  assert.equal(formatCountdown(3820, 100), '1h02m')
  assert.equal(formatCountdown(93700, 100), '1d2h')
})
