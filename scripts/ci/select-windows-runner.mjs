import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

export const SELF_HOSTED_WINDOWS_LABEL = 'uniclipboard-desktop-windows-x64'
export const HOSTED_WINDOWS_LABEL = 'windows-latest'

export function selectWindowsRunner(runners) {
  const available = runners.some(
    runner =>
      runner.status === 'online' &&
      runner.busy === false &&
      runner.labels?.some(label => label.name === SELF_HOSTED_WINDOWS_LABEL)
  )

  return available
    ? { runner: SELF_HOSTED_WINDOWS_LABEL, source: 'self-hosted' }
    : { runner: HOSTED_WINDOWS_LABEL, source: 'github-hosted' }
}

export async function resolveWindowsRunner({ apiUrl, repository, token, fetchImpl = fetch }) {
  if (!token) {
    return { runner: HOSTED_WINDOWS_LABEL, source: 'github-hosted', reason: 'missing-token' }
  }

  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), 10_000)
  try {
    const response = await fetchImpl(`${apiUrl}/repos/${repository}/actions/runners`, {
      signal: controller.signal,
      headers: {
        Accept: 'application/vnd.github+json',
        Authorization: `Bearer ${token}`,
        'X-GitHub-Api-Version': '2022-11-28',
      },
    })
    if (!response.ok) throw new Error(`GitHub API returned ${response.status}`)

    const payload = await response.json()
    return { ...selectWindowsRunner(payload.runners ?? []), reason: 'runner-status' }
  } catch (error) {
    console.warn(`Unable to read self-hosted runner status; using ${HOSTED_WINDOWS_LABEL}.`)
    console.warn(error instanceof Error ? error.message : String(error))
    return { runner: HOSTED_WINDOWS_LABEL, source: 'github-hosted', reason: 'api-error' }
  } finally {
    clearTimeout(timeout)
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (!process.env.GITHUB_OUTPUT) throw new Error('GITHUB_OUTPUT is required')

  const result = await resolveWindowsRunner({
    apiUrl: process.env.GITHUB_API_URL ?? 'https://api.github.com',
    repository: process.env.GITHUB_REPOSITORY,
    token: process.env.RUNNER_STATUS_TOKEN,
  })
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    `runner=${result.runner}\nsource=${result.source}\nreason=${result.reason}\n`
  )
  console.log(`Selected ${result.runner} (${result.reason}).`)
}
