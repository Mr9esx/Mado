const round = value => Math.round(value * 10) / 10
const gibibytes = value => `${(value / 1024 ** 3).toFixed(1)} GB`

function rate(current, previous, elapsedMs) {
  if (!Number.isFinite(current) || !Number.isFinite(previous) || current < previous || !Number.isFinite(elapsedMs) || elapsedMs <= 0) return null
  return { value: Math.round((current - previous) * 1000 / elapsedMs) }
}

export function projectSystemSnapshot(snapshot, previousNetwork, sampledAtMs) {
  const cpuPercent = Number.isFinite(snapshot.cpuUsagePercent)
    ? round(snapshot.cpuUsagePercent)
    : snapshot.cpuPercent
  const memoryTotal = snapshot.memoryTotalBytes
  const memoryUsed = Number.isFinite(snapshot.memoryUsedBytes)
    ? snapshot.memoryUsedBytes
    : memoryTotal - snapshot.memoryAvailableBytes
  const memoryPercent = memoryTotal > 0
    ? round(memoryUsed / memoryTotal * 100)
    : snapshot.memoryPercent
  const elapsedMs = sampledAtMs - (previousNetwork?.sampledAtMs ?? sampledAtMs)
  const currentNetwork = {
    receivedBytes: snapshot.networkReceivedBytes,
    transmittedBytes: snapshot.networkTransmittedBytes,
    sampledAtMs,
  }
  const memoryDetails = memoryTotal > 0
    ? { used: gibibytes(memoryUsed), total: gibibytes(memoryTotal) }
    : undefined
  const outputs = {
    cpuUsage: cpuPercent == null ? null : { value: cpuPercent },
    memoryUsage: memoryPercent == null ? null : {
      value: memoryPercent,
      ratio: memoryTotal > 0 ? memoryUsed / memoryTotal : memoryPercent / 100,
      ...(memoryDetails ? { details: memoryDetails } : {}),
    },
    systemDisk: Number.isFinite(snapshot.systemDiskUsedPercent)
      ? { value: round(snapshot.systemDiskUsedPercent), ratio: snapshot.systemDiskUsedPercent / 100 }
      : null,
    downloadRate: previousNetwork ? rate(snapshot.networkReceivedBytes, previousNetwork.receivedBytes, elapsedMs) : null,
    uploadRate: previousNetwork ? rate(snapshot.networkTransmittedBytes, previousNetwork.transmittedBytes, elapsedMs) : null,
    uptime: { value: Math.floor(snapshot.uptimeSeconds / 60) },
  }
  return { outputs, network: currentNetwork }
}
