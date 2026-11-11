import { defineModule } from '@mado/sdk'
import { collectBilibili } from './client.mjs'

export default defineModule({
  async collect(settings, context) {
    return collectBilibili(settings, context, context.state)
  },
  async collectBatch(instances, context) {
    const results = []
    for (const { id, settings, state } of instances) {
      try {
        const result = await collectBilibili(settings, context.forInstance(id), state)
        results.push({ instanceId: id, ...result })
      } catch (error) {
        results.push({ instanceId: id, error: error.message })
      }
    }
    return { results }
  },
})
