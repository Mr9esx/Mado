import { defineModule } from '@tdisplay/sdk'

export default defineModule({
  async collect(_, context) {
    const { cpuPercent, memoryPercent, uptimeSeconds } = await context.system.read('system')
    return {
      outputs: {
        cpuUsage: cpuPercent == null ? null : { value: cpuPercent },
        memoryUsage: memoryPercent == null ? null : { value: memoryPercent, ratio: memoryPercent / 100 },
        uptime: { value: Math.floor(uptimeSeconds / 60) },
      },
    }
  },
})
