import assert from 'node:assert/strict'
import { after, before, test } from 'node:test'
import { CompletionContext } from '@codemirror/autocomplete'
import { javascript } from '@codemirror/lang-javascript'
import { EditorState } from '@codemirror/state'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'

let server
before(async () => { server = await createServer({ server: { middlewareMode: true, hmr: false } }) })
after(async () => { await server?.close() })

const variables = {
  fields: [
    { key: 'REPO', label: '仓库', description: '填写 owner/repo', required: true, secret: false, value: '' },
    { key: 'REQUIRED_TOKEN', label: '私有令牌', required: true, secret: true },
    { key: 'OPTIONAL_TOKEN', label: '可选令牌', required: false, secret: true },
    { key: 'NEW_TOKEN', label: '备用令牌', required: false, secret: true },
  ],
  configured: { REPO: false, REQUIRED_TOKEN: true, OPTIONAL_TOKEN: true, NEW_TOKEN: false },
}

test('renders released variable metadata in order and never fills password fields', async () => {
  const { CollectorVariableForm } = await server.ssrLoadModule('/src/CollectorVariableForm.tsx')
  const markup = renderToStaticMarkup(createElement(CollectorVariableForm, {
    variables, changes: {}, onChange() {}, onSave() {}, onClose() {},
  }))
  assert.match(markup, /仓库.*填写 owner\/repo.*私有令牌.*可选令牌.*备用令牌/)
  assert.equal((markup.match(/type="password"[^>]*value=""/g) ?? []).length, 3)
  assert.equal((markup.match(/>已配置</g) ?? []).length, 2)
  assert.equal((markup.match(/>未配置</g) ?? []).length, 1)
  assert.match(markup, /aria-label="清除可选令牌"/)
  assert.doesNotMatch(markup, /aria-label="清除私有令牌"/)
  assert.match(markup, /<button[^>]*disabled=""[^>]*type="submit"|<button[^>]*type="submit"[^>]*disabled=""/)
})

test('retains configured secrets, blocks empty required entries, and clears optional fields', async () => {
  const { collectorVariableChanges, canSaveCollectorVariables } = await server.ssrLoadModule('/src/CollectorVariableForm.tsx')
  const entered = collectorVariableChanges({}, 'REPO', 'owner/repo')
  assert.equal(canSaveCollectorVariables(variables, { REPO: '  ' }), false)
  assert.equal(canSaveCollectorVariables(variables, entered), true)
  const edited = collectorVariableChanges(entered, 'OPTIONAL_TOKEN', 'replacement')
  assert.deepEqual(collectorVariableChanges(edited, 'OPTIONAL_TOKEN', undefined), { REPO: 'owner/repo' })
  const cleared = collectorVariableChanges(entered, 'OPTIONAL_TOKEN', '')
  assert.deepEqual(cleared, { REPO: 'owner/repo', OPTIONAL_TOKEN: '' })
  assert.equal(canSaveCollectorVariables(variables, cleared), true)
  assert.equal(canSaveCollectorVariables(variables, { ...cleared, REQUIRED_TOKEN: '' }), false)
})

test('completes variable keys and declaration metadata alongside legacy settings', async () => {
  const { collectorCompletion } = await server.ssrLoadModule('/src/CollectorCode.tsx')
  const draft = {
    id: 'module', entry: 'index.mjs', sdkApiVersion: 2, files: {},
    definition: JSON.stringify({ settings: { REPO: { label: '仓库', required: true }, oldKey: { label: '历史参数' } } }),
  }
  const complete = (doc, file = draft.entry, current = draft) => collectorCompletion(
    new CompletionContext(EditorState.create({ doc, extensions: [javascript()] }), doc.length, true), file, current,
  )
  const access = complete('settings.RE', 'helper.mjs')
  assert.equal(access.from, 'settings.'.length)
  assert.deepEqual(access.options.map(({ label, detail }) => [label, detail]), [['REPO', '仓库'], ['oldKey', '历史参数']])
  assert.ok(complete('export default defineModule({ settings: { RE').options.some(item => item.label === 'REPO'))
  const source = 'defineModule({ settings: { REPO: { label: "仓库", required: true } }, collect(settings) { settings.RE'
  assert.ok(complete(source, draft.entry, { ...draft, definition: '{}' }).options.some(item => item.label === 'REPO'))
  const metadata = complete('defineModule({ settings: { REPO: { req')
  assert.ok(metadata.options.some(item => item.label === 'required'))
  const legacy = { ...draft, sdkApiVersion: 1, definition: '{"inputs":{"source":{"label":"来源"}}}' }
  assert.ok(complete('inputs.so', draft.entry, legacy).options.some(item => item.label === 'source'))
})
