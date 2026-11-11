import { defineModule } from '@mado/sdk'

export default defineModule({
  async collect(_, context) {
    const snapshot = await context.system.read('system')
    const round = value => Math.round(value * 10) / 10
    const cpuPercent = Number.isFinite(snapshot.cpuUsagePercent)
      ? round(snapshot.cpuUsagePercent)
      : snapshot.cpuPercent
    const memoryPercent = snapshot.memoryTotalBytes > 0
      ? round(snapshot.memoryUsedBytes / snapshot.memoryTotalBytes * 100)
      : snapshot.memoryPercent
    return {
      outputs: {
        cpuUsage: cpuPercent == null ? null : { value: cpuPercent },
        memoryUsage: memoryPercent == null ? null : { value: memoryPercent, ratio: memoryPercent / 100 },
        uptime: { value: Math.floor(snapshot.uptimeSeconds / 60) },
      },
    }
  },
})
