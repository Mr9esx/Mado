import { defineModule } from '@mado/sdk'
import { collectBilibili } from './client.mjs'

export default defineModule({
  async collect(settings, context) {
    return collectBilibili(settings, context, context.state)
  },
})
