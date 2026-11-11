import assert from 'node:assert/strict'
import test, { after, before } from 'node:test'
import { createServer } from 'vite'

let server
before(async () => { server = await createServer({ server: { middlewareMode: true, hmr: false } }) })
after(async () => { await server?.close() })

test('authorization dialog copy follows the action that requests consent', async () => {
  const { authorizationDescription } = await server.ssrLoadModule('/src/CollectorAuthorization.tsx')
  assert.match(authorizationDescription('test'), /本次试跑将访问以下资源/)
  assert.match(authorizationDescription('publish'), /发布后，应用会按以下范围代脚本访问资源/)
  assert.match(authorizationDescription('enable'), /启用后，采集器会按以下范围访问资源/)
})
