import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import ts from 'typescript'
import { describe, expect, it } from 'vitest'

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    if (entry.name === '__tests__' || entry.name === 'test') return []
    const path = join(directory, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(ts|tsx)$/.test(path) && !/\.test\./.test(path) ? [path] : []
  })
}

describe('diagnostics provider boundary', () => {
  it('keeps SDK imports and private provider access out of application code', () => {
    const violations: string[] = []
    for (const path of sourceFiles(join(process.cwd(), 'src'))) {
      const name = relative(process.cwd(), path).replace(/\\/g, '/')
      if (name === 'src/observability/sentry.ts') continue
      const source = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest)
      for (const statement of source.statements) {
        if (!ts.isImportDeclaration(statement) && !ts.isExportDeclaration(statement)) continue
        const specifier = statement.moduleSpecifier
        if (!specifier || !ts.isStringLiteral(specifier)) continue
        const sdkImport = specifier.text.startsWith('@sentry/')
        const privateProvider = /(?:^|\/)sentry$/.test(specifier.text)
        if (sdkImport || (privateProvider && name !== 'src/observability/diagnostics.ts')) {
          violations.push(`${name}: ${specifier.text}`)
        }
      }
    }
    expect(violations).toEqual([])
  })
})
