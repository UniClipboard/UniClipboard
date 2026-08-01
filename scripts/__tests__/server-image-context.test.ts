import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const projectRoot = path.resolve(__dirname, '../..')

function localCopySources(dockerfile: string): string[] {
  return dockerfile
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.startsWith('COPY ') && !line.includes('--from='))
    .flatMap(line => line.slice('COPY '.length).trim().split(/\s+/).slice(0, -1))
    .map(source => source.replace(/\/$/, ''))
}

describe('server image build context', () => {
  it('only copies paths that exist in the repository', () => {
    const dockerfile = fs.readFileSync(path.join(projectRoot, 'deploy/vps/Dockerfile'), 'utf8')
    const missingSources = localCopySources(dockerfile).filter(
      source => !fs.existsSync(path.join(projectRoot, source))
    )

    expect(missingSources).toEqual([])
  })
})
