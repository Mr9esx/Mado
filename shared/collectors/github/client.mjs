const repositoryName = /^[A-Za-z0-9_.-]+$/

export function normalizeRepository(input) {
  if (typeof input !== 'string') throw new Error('请填写有效的 GitHub 仓库。')
  let value = input.trim()
  if (/^https?:\/\//i.test(value)) {
    const match = value.match(/^https:\/\/github\.com\/([^/?#]+)\/([^/?#]+)\/?$/i)
    if (!match) throw new Error('仅支持 github.com 上的仓库。')
    value = `${match[1]}/${match[2].replace(/\.git$/i, '')}`
  }
  const parts = value.split('/')
  if (parts.length !== 2 || parts.some(part => !repositoryName.test(part) || part === '.' || part === '..')) {
    throw new Error('仓库格式应为 owner/repo。')
  }
  return `${parts[0]}/${parts[1]}`
}

export function normalizeRunState(run) {
  if (run?.status === 'in_progress') return 'running'
  if (['queued', 'waiting', 'requested', 'pending'].includes(run?.status)) return 'queued'
  if (run?.status !== 'completed') return 'unknown'
  if (['success', 'neutral', 'skipped'].includes(run.conclusion)) return 'success'
  if (['failure', 'timed_out', 'action_required', 'startup_failure'].includes(run.conclusion)) return 'failure'
  if (['cancelled', 'stale'].includes(run.conclusion)) return 'cancelled'
  return 'unknown'
}

async function getJson(context, path, token) {
  const headers = { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' }
  if (token) headers.Authorization = `Bearer ${token}`
  const response = await context.http.request('githubApi', { method: 'GET', path, headers })
  if (!Number.isInteger(response?.status) || response.status < 200 || response.status >= 300) {
    if (response?.status === 403 || response?.status === 429) {
      throw new Error('GitHub API 请求受限，请检查令牌权限或稍后重试。')
    }
    if (response?.status === 401) throw new Error('GitHub 令牌无效或已过期。')
    throw new Error(`GitHub API 请求失败（HTTP ${Number.isInteger(response?.status) ? response.status : '未知'}）。`)
  }
  try { return JSON.parse(response.text) } catch { throw new Error('GitHub API 返回了无效数据。') }
}

function countFromSearch(result) {
  if (!Number.isSafeInteger(result?.total_count) || result.total_count < 0) {
    throw new Error('GitHub API 返回的数量无效。')
  }
  return { value: result.total_count }
}

export async function collectGithub(settings, context) {
  const repository = normalizeRepository(settings?.GITHUB_REPOSITORY)
  const [pullRequestResult, issueResult, actionsResult] = await Promise.all([
    getJson(context, `/search/issues?q=repo%3A${encodeURIComponent(repository)}+is%3Apr+is%3Aopen&per_page=1`, settings?.GITHUB_TOKEN),
    getJson(context, `/search/issues?q=repo%3A${encodeURIComponent(repository)}+is%3Aissue+is%3Aopen&per_page=1`, settings?.GITHUB_TOKEN),
    getJson(context, `/repos/${repository.split('/').map(encodeURIComponent).join('/')}/actions/runs?per_page=1`, settings?.GITHUB_TOKEN),
  ])
  if (!Array.isArray(actionsResult?.workflow_runs)) throw new Error('GitHub API 返回的 Actions 数据无效。')
  const latestRun = actionsResult.workflow_runs[0]
  const ci = latestRun == null
    ? { state: 'unknown', detail: '暂无运行记录' }
    : {
        state: normalizeRunState(latestRun),
        ...(typeof latestRun.name === 'string' && latestRun.name.trim()
          ? { detail: latestRun.name.trim().slice(0, 80) }
          : {}),
      }
  return {
    outputs: {
      pullRequests: countFromSearch(pullRequestResult),
      issues: countFromSearch(issueResult),
      ci,
    },
  }
}
