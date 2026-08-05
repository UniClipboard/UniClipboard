import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, test } from 'vitest'

const workflowPath = path.resolve(
  import.meta.dirname,
  '../../.github/workflows/issue-lifecycle.yml'
)
const releaseWorkflowPath = path.resolve(import.meta.dirname, '../../.github/workflows/release.yml')

function readWorkflow(): string {
  return fs.readFileSync(workflowPath, 'utf8')
}

function readReleaseWorkflow(): string {
  return fs.readFileSync(releaseWorkflowPath, 'utf8')
}

describe('Issue lifecycle workflow', () => {
  test('keeps linked issues open until an authorized verification command', () => {
    const workflow = readWorkflow()

    expect(workflow).toContain('pull_request:')
    expect(workflow).toContain('issue_comment:')
    expect(workflow).toContain('release:')
    expect(workflow).toContain('status:ready-for-test')
    expect(workflow).toContain('status:verified')
    expect(workflow).toContain("state: 'closed'")
    expect(workflow).toContain('getCollaboratorPermissionLevel')
    expect(workflow).toContain("['admin', 'maintain', 'write', 'triage']")
  })

  test('rejects pull request text that would let GitHub close an issue on merge', () => {
    const workflow = readWorkflow()

    expect(workflow).toContain('closingReferences')
    expect(workflow).toContain('closing keywords')
    expect(workflow).toContain('Related to #123')
    expect(workflow).toContain('(?:related(?:\\s+to)?|refs?)\\s*:?\\s+#(\\d+)\\b')
  })

  test('records the release that must be verified before accepting the command', () => {
    const workflow = readWorkflow()

    expect(workflow).toContain('verification-release=')
    expect(workflow).toContain('/verify vX.Y.Z')
    expect(workflow).toContain('listComments')
  })

  test('notifies issues for releases published by the release workflow itself', () => {
    const workflow = readWorkflow()
    const releaseWorkflow = readReleaseWorkflow()

    expect(workflow).toContain('repository_dispatch:')
    expect(workflow).toContain('issue-lifecycle-release')
    expect(releaseWorkflow).toContain('-f "event_type=issue-lifecycle-release"')
  })
})
