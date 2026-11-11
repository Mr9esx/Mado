import assert from 'node:assert/strict'
import test from 'node:test'
import { projectSystemSnapshot } from '../shared/collectors/system/projection.mjs'

const snapshot = (overrides = {}) => ({
  cpuUsagePercent: 42.56,
  memoryUsedBytes: 7 * 1024 ** 3,
  memoryAvailableBytes: 9 * 1024 ** 3,
  memoryTotalBytes: 16 * 1024 ** 3,
  systemDiskUsedPercent: 73.25,
  networkReceivedBytes: 5000,
  networkTransmittedBytes: 9000,
  uptimeSeconds: 7200,
  ...overrides,
})

test('projects current CPU, memory details, disk usage, and uptime while preserving cpuUsage', () => {
  const { outputs } = projectSystemSnapshot(snapshot(), null, 10_000)
  assert.deepEqual(outputs.cpuUsage, { value: 42.6 })
  assert.deepEqual(outputs.memoryUsage, {
    value: 43.8, ratio: 7 / 16,
    details: { used: '7.0 GB', total: '16.0 GB' },
  })
  assert.deepEqual(outputs.systemDisk, { value: 73.3, ratio: 0.7325 })
  assert.deepEqual(outputs.uptime, { value: 120 })
})

test('network rates are unavailable on first sample then project upload and download rates', () => {
  const first = projectSystemSnapshot(snapshot(), null, 10_000)
  assert.equal(first.outputs.downloadRate, null)
  assert.equal(first.outputs.uploadRate, null)
  const current = snapshot({ networkReceivedBytes: 9500, networkTransmittedBytes: 11000 })
  const next = projectSystemSnapshot(current, first.network, 12_000)
  assert.deepEqual(next.outputs.downloadRate, { value: 2250 })
  assert.deepEqual(next.outputs.uploadRate, { value: 1000 })
  assert.deepEqual(next.network, { receivedBytes: 9500, transmittedBytes: 11000, sampledAtMs: 12_000 })
})

test('network counter reset and nonpositive interval never produce negative or infinite rates', () => {
  const previous = { receivedBytes: 9000, transmittedBytes: 11_000, sampledAtMs: 10_000 }
  const reset = projectSystemSnapshot(snapshot({ networkReceivedBytes: 2, networkTransmittedBytes: 3 }), previous, 12_000)
  assert.equal(reset.outputs.downloadRate, null)
  assert.equal(reset.outputs.uploadRate, null)
  const unchanged = projectSystemSnapshot(snapshot({ networkReceivedBytes: 9000, networkTransmittedBytes: 11_000 }), previous, 10_000)
  assert.equal(unchanged.outputs.downloadRate, null)
  assert.equal(unchanged.outputs.uploadRate, null)
})
