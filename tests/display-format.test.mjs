import assert from 'node:assert/strict'
import test from 'node:test'
import { formatMetric } from '../src/displayFormat.ts'

test('keeps readings within four characters without losing ordinary decimals', () => {
  for (const [input, expected] of [[0, '0'], [81, '81'], [248, '248'], [12.3, '12.3'], [1.25, '1.25'], [1234, '1.2k'], [9999, '10k'], [-1234, '-1k']]) {
    assert.equal(formatMetric(input), expected)
  }
  for (const input of [999.99, -99.99, 999999, -999999, 1e20, -1e20]) {
    assert.ok(formatMetric(input).length <= 4, String(input))
  }
})

test('distinguishes missing or invalid readings from a real zero', () => {
  for (const input of [undefined, null, NaN, Infinity, -Infinity]) assert.equal(formatMetric(input), '—')
  assert.equal(formatMetric(0), '0')
})

test('digit rolling right aligns place values and leaves matching positions still', async () => {
  const { digitRollColumns } = await import('../src/displayFormat.ts')
  assert.deepEqual(digitRollColumns('109', '110', 1), [['1'], ['0', '1'], ['9', '0']])
  assert.deepEqual(digitRollColumns('100', '99', -1), [['0', '9'], ['0', '9']])
  assert.deepEqual(digitRollColumns('1.2', '1.4', 1), [['1'], ['.'], ['2', '3', '4']])
})
