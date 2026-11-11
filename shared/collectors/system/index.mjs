import { defineModule } from '@mado/sdk'
import { projectSystemSnapshot } from './projection.mjs'

export default defineModule({
  async collect(_, context) {
    const snapshot = await context.system.read('system')
    const previousNetwork = context.state?.network ?? null
    const projected = projectSystemSnapshot(snapshot, previousNetwork, context.sampledAtUnixMs)
    return { outputs: projected.outputs, state: { network: projected.network } }
  },
})
