import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const projectRoot = path.resolve(import.meta.dirname, '../..')

function readWorkflow(): string {
  return fs.readFileSync(path.join(projectRoot, '.github/workflows/security-audit.yml'), 'utf8')
}

describe('security audit issue labeling', () => {
  it('labels every RustSec issue after the audit action creates it', () => {
    const workflow = readWorkflow()

    expect(workflow).toContain('security-audit')
    expect(workflow).toContain('Label RustSec issues')
    expect(workflow).toContain('gh issue list')
    expect(workflow).toContain('startswith("RUSTSEC-")')
    expect(workflow).toContain('--add-label "$LABEL"')
  })
})
