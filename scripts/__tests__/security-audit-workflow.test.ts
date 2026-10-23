import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import picomatch from 'picomatch'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

const projectRoot = path.resolve(import.meta.dirname, '../..')
const workflow = parse(
  fs.readFileSync(path.join(projectRoot, '.github/workflows/security-audit.yml'), 'utf8')
)
const rust = workflow.jobs['cargo-audit']
const scheduledAudit = rust.steps.find(step => step.name === 'Audit Rust dependencies on schedule')
const detector = workflow.jobs.changes.steps.find(step => step.id === 'filter')
const projectSelector = workflow.jobs.changes.steps.find(step => step.id === 'projects')

function selectedProjects(files: string[], all = false) {
  const filters = parse(detector.with.filters) as Record<string, string[]>
  const changed = Object.entries(filters)
    .filter(([, rules]) => files.some(file => picomatch(rules, { dot: true })(file)))
    .map(([name]) => name)
  let projects: { name: string; directory: string; ignore: string }[] = []
  new Function('core', 'process', projectSelector.with.script)(
    {
      setOutput: (_name: string, value: string) => {
        projects = JSON.parse(value)
      },
    },
    { env: { ALL_PROJECTS: String(all), CHANGED_PROJECTS: JSON.stringify(changed) } }
  )
  return { changed, projects }
}

describe('security audit reporting without issues', () => {
  it.each([
    ['Cargo.lock', ['rust'], []],
    ['crates/uc-platform/Cargo.toml', ['rust'], []],
    ['bun.lock', ['root'], ['.']],
    ['docs-site/bun.lock', ['docs-site'], ['docs-site']],
    ['workers/update-server/package.json', ['update-server'], ['workers/update-server']],
    ['docs/README.md', [], []],
  ])('audits only consumers of %s', (file, changed, directories) => {
    const result = selectedProjects([file])
    expect(result.changed).toEqual(changed)
    expect(result.projects.map(project => project.directory)).toEqual(directories)
  })

  it('selects all audits when their workflow changes and all Bun projects for full runs', () => {
    const result = selectedProjects(['.github/workflows/security-audit.yml'])
    expect(result.changed).toEqual(['rust', 'root', 'docs-site', 'update-server'])
    expect(result.projects).toEqual(selectedProjects([], true).projects)
    expect(rust.needs).toBe('changes')
    expect(rust.if).toContain("needs.changes.outputs.rust == 'true'")
    expect(rust.if).toContain("github.event_name == 'schedule'")
    expect(rust.if).toContain("github.event_name == 'workflow_dispatch'")
    expect(workflow.jobs['bun-audit'].if).toBe("needs.changes.outputs.projects != '[]'")
    expect(workflow.jobs['bun-audit'].strategy.matrix.project).toBe(
      '${{ fromJSON(needs.changes.outputs.projects) }}'
    )
  })

  it('keeps workflow triggers aligned with the component filters and routes mixed updates', () => {
    const filters = parse(detector.with.filters) as Record<string, string[]>
    const inputs = [...new Set(Object.values(filters).flat())].sort()
    expect([...workflow.on.pull_request.paths].sort()).toEqual(inputs)
    expect([...workflow.on.push.paths].sort()).toEqual(inputs)
    expect(
      selectedProjects(['Cargo.lock', 'bun.lock', 'docs-site/bun.lock']).projects.map(
        project => project.name
      )
    ).toEqual(['root', 'docs-site'])
    expect(detector.if).toBe("github.event_name == 'pull_request' || github.event_name == 'push'")
    expect(projectSelector.env.ALL_PROJECTS).toBe(
      "${{ github.event_name == 'schedule' || github.event_name == 'workflow_dispatch' }}"
    )
  })
  it('retains triggers and check reporting outside scheduled runs without issue permissions', () => {
    expect(Object.keys(workflow.on).sort()).toEqual([
      'pull_request',
      'push',
      'schedule',
      'workflow_dispatch',
    ])
    expect(workflow.on.schedule).toEqual([{ cron: '0 0 * * *' }])
    expect(rust.permissions).toEqual({ contents: 'read', checks: 'write' })
    const check = rust.steps.find(step => step.uses?.startsWith('rustsec/audit-check@'))
    expect(check.if).toBe("github.event_name != 'schedule'")
    expect(check.with.token).toBe('${{ secrets.GITHUB_TOKEN }}')
    expect(scheduledAudit.if).toBe("github.event_name == 'schedule'")
    expect(scheduledAudit.shell).toBe('bash')
    expect(rust.steps.some(step => /gh issue|reportIssues|createIssue/.test(step.run ?? ''))).toBe(
      false
    )
    expect(rust['continue-on-error']).toBeUndefined()
    expect(scheduledAudit['continue-on-error']).toBeUndefined()
  })

  it.each([0, 1, 2])(
    'preserves audit exit status %i, logs, and summary without GitHub access',
    status => {
      const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'security-audit-'))
      try {
        const summary = path.join(directory, 'summary.md')
        fs.writeFileSync(
          path.join(directory, 'cargo'),
          `#!/bin/bash\n[ "$*" = audit ] || exit 99\necho 'audit diagnostic'\necho 'audit stderr' >&2\nexit ${status}\n`,
          { mode: 0o755 }
        )
        const result = spawnSync('bash', ['-c', scheduledAudit.run], {
          cwd: projectRoot,
          env: {
            PATH: `${directory}:/usr/bin:/bin`,
            RUNNER_TEMP: directory,
            GITHUB_STEP_SUMMARY: summary,
          },
          encoding: 'utf8',
        })
        expect(result.error).toBeUndefined()
        expect(result.status).toBe(status)
        expect(result.stdout).toContain('audit diagnostic')
        expect(result.stdout).toContain('audit stderr')
        const report = fs.readFileSync(summary, 'utf8')
        expect(report).toContain(`Exit status: ${status}`)
        expect(report).toContain('audit diagnostic')
        expect(report).toContain('audit stderr')
      } finally {
        fs.rmSync(directory, { recursive: true, force: true })
      }
    }
  )

  it('preserves production Bun audits for all three projects', () => {
    const bun = workflow.jobs['bun-audit']
    expect(bun.strategy['fail-fast']).toBe(false)
    const projects = selectedProjects([], true).projects
    expect(projects.map(project => project.directory)).toEqual([
      '.',
      'docs-site',
      'workers/update-server',
    ])
    expect(projects.find(project => project.name === 'root')?.ignore).toBe('')
    expect(projects.find(project => project.name === 'docs-site')?.ignore).toContain(
      '--ignore=GHSA-'
    )
    expect(bun.steps.at(-1).run).toBe('bun audit --production ${{ matrix.project.ignore }}')
  })
})
