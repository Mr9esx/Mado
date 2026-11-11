import { defineModule } from '@mado/sdk'
import { collectGithub } from './client.mjs'

export default defineModule({
  async collect(settings, context) {
    return collectGithub(settings, context)
  },
})
