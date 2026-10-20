import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

const projectRoot = path.resolve(import.meta.dirname, '../..')
const workflow = parse(
  fs.readFileSync(path.join(projectRoot, '.github/workflows/security-audit.yml'), 'utf8')
)
const rust = workflow.jobs['cargo-audit']
const scheduledAudit = rust.steps.find(step => step.name === 'Audit Rust dependencies on schedule')

describe('security audit reporting without issues', () => {
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
    expect(bun.strategy.matrix.project.map(project => project.directory)).toEqual([
      '.',
      'docs-site',
      'workers/update-server',
    ])
    expect(bun.steps.at(-1).run).toBe('bun audit --production ${{ matrix.project.ignore }}')
  })
})
