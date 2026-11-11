import assert from 'node:assert/strict'
import test from 'node:test'
import { collectGithub, normalizeRepository, normalizeRunState } from '../shared/collectors/github/client.mjs'

const response = (value, status = 200) => ({ status, text: JSON.stringify(value) })
const api = (overrides = {}) => {
  const calls = []
  const context = {
    http: {
      async request(alias, options) {
        calls.push({ alias, ...options })
        const reply = overrides[options.path]
        if (reply instanceof Error) throw reply
        return typeof reply === 'function' ? reply(options) : reply ?? response({})
      },
    },
  }
  return { context, calls }
}

const paths = {
  prs: '/search/issues?q=repo%3Aopenai%2Fcodex+is%3Apr+is%3Aopen&per_page=1',
  issues: '/search/issues?q=repo%3Aopenai%2Fcodex+is%3Aissue+is%3Aopen&per_page=1',
  runs: '/repos/openai/codex/actions/runs?per_page=1',
}

 test('normalizes repository slug and official GitHub repository URL only', () => {
  assert.equal(normalizeRepository('openai/codex'), 'openai/codex')
  assert.equal(normalizeRepository('https://github.com/openai/codex.git/'), 'openai/codex')
  for (const input of ['', 'openai', 'openai/codex/issues', 'https://evil.example/openai/codex', 'openai/name with spaces']) {
    assert.throws(() => normalizeRepository(input))
  }
})

test('maps GitHub Actions run status and conclusion to a compact state', () => {
  assert.equal(normalizeRunState({ status: 'in_progress' }), 'running')
  assert.equal(normalizeRunState({ status: 'queued' }), 'queued')
  assert.equal(normalizeRunState({ status: 'completed', conclusion: 'success' }), 'success')
  assert.equal(normalizeRunState({ status: 'completed', conclusion: 'failure' }), 'failure')
  assert.equal(normalizeRunState({ status: 'completed', conclusion: 'cancelled' }), 'cancelled')
  assert.equal(normalizeRunState({ status: 'mystery' }), 'unknown')
})

test('collects open PRs, open issues, and latest Actions state with optional bearer token', async () => {
  const { context, calls } = api({
    [paths.prs]: response({ total_count: 7 }),
    [paths.issues]: response({ total_count: 11 }),
    [paths.runs]: response({ workflow_runs: [{ status: 'completed', conclusion: 'success', name: 'CI' }] }),
  })
  const result = await collectGithub({ GITHUB_REPOSITORY: 'openai/codex', GITHUB_TOKEN: 'ghp_test_secret' }, context)
  assert.deepEqual(result, {
    outputs: {
      pullRequests: { value: 7 },
      issues: { value: 11 },
      ci: { state: 'success', detail: 'CI' },
    },
  })
  assert.equal(calls.length, 3)
  assert.ok(calls.every(call => call.alias === 'githubApi' && (call.method ?? 'GET') === 'GET'))
  assert.ok(calls.every(call => call.headers.Authorization === 'Bearer ghp_test_secret'))
  assert.ok(!JSON.stringify(result).includes('ghp_test_secret'))
})

test('does not send Authorization when token is empty', async () => {
  const { context, calls } = api({
    [paths.prs]: response({ total_count: 1 }),
    [paths.issues]: response({ total_count: 2 }),
    [paths.runs]: response({ workflow_runs: [] }),
  })
  await collectGithub({ GITHUB_REPOSITORY: 'openai/codex', GITHUB_TOKEN: '' }, context)
  assert.ok(calls.every(call => !('Authorization' in call.headers)))
})

test('rejects API errors and rate limits instead of showing zero counts', async () => {
  for (const status of [401, 403, 429, 500]) {
    const { context } = api({ [paths.prs]: response({ total_count: 0 }, status) })
    await assert.rejects(() => collectGithub({ GITHUB_REPOSITORY: 'openai/codex' }, context))
  }
})
