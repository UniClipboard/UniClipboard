import { readFileSync, readdirSync } from 'node:fs'
import path from 'node:path'
import postcss from 'postcss'
import ts from 'typescript'
import { describe, expect, it } from 'vitest'
import { cn } from '@/lib/utils'

const sourceRoot = path.resolve(import.meta.dirname, '../..')
const ownedControls = new Set([
  'Button',
  'Input',
  'Label',
  'SelectTrigger',
  'SelectItem',
  'DialogTitle',
  'DialogDescription',
  'AlertDialogTitle',
  'AlertDialogDescription',
  'SheetTitle',
  'SheetDescription',
  'TooltipContent',
  'ContextMenuItem',
  'ContextMenuCheckboxItem',
  'ContextMenuRadioItem',
  'ContextMenuShortcut',
  'ContextMenuLabel',
  'ContextMenuSubTrigger',
  'Badge',
  'Kbd',
])

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(directory, entry.name)
    if (entry.isDirectory()) return entry.name === '__tests__' ? [] : sourceFiles(file)
    return /\.(tsx?|css)$/.test(file) && !/\.(test|spec)\./.test(file) ? [file] : []
  })
}

describe('main-window typography contract', () => {
  it('keeps semantic text roles in the font-size group without removing colors', () => {
    expect(cn('text-ui-body text-muted-foreground', 'text-ui-caption')).toBe(
      'text-muted-foreground text-ui-caption'
    )
    expect(cn('text-ui-body-relaxed', 'text-ui-body')).toBe('text-ui-body')
    expect(cn('text-sm text-destructive', 'text-ui-body')).toBe('text-destructive text-ui-body')
  })

  it('prevents pages and shared components from introducing independent typography', () => {
    const violations: string[] = []
    const files = ['pages', 'layouts', 'components'].flatMap(dir =>
      sourceFiles(path.join(sourceRoot, dir))
    )
    for (const file of files) {
      const source = readFileSync(file, 'utf8')
      const relative = path.relative(sourceRoot, file)
      if (file.endsWith('.css')) {
        postcss.parse(source).walkDecls(declaration => {
          if (/^(font|font-size|line-height|letter-spacing)$/.test(declaration.prop)) {
            violations.push(`${relative}: ${declaration.prop}`)
          }
        })
        continue
      }
      const ast = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true)
      function visit(node: ts.Node) {
        if (
          ts.isPropertyAssignment(node) &&
          ts.isObjectLiteralExpression(node.parent) &&
          ts.isJsxExpression(node.parent.parent) &&
          ts.isJsxAttribute(node.parent.parent.parent) &&
          node.parent.parent.parent.name.getText(ast) === 'style' &&
          ['font', 'fontSize', 'lineHeight', 'letterSpacing'].includes(node.name.getText(ast))
        ) {
          violations.push(`${relative}: inline ${node.name.getText(ast)}`)
        }
        if (
          ts.isStringLiteralLike(node) ||
          ts.isTemplateHead(node) ||
          ts.isTemplateMiddle(node) ||
          ts.isTemplateTail(node)
        ) {
          const illegal = node.text.match(
            /\b(?:text-(?:xs|sm|base|lg|xl|[2-9]xl)(?![\w-])|text-\[(?:[\d.]|length:)|leading-[\w[]|tracking-[\w[])|\b(?:sm|md|lg|xl):text-ui-/g
          )
          if (illegal) violations.push(`${relative}: ${illegal.join(', ')}`)
        }
        if (
          (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
          ownedControls.has(node.tagName.getText(ast))
        ) {
          for (const attribute of node.attributes.properties) {
            if (
              ts.isJsxAttribute(attribute) &&
              attribute.name.getText(ast) === 'className' &&
              /\btext-ui-/.test(attribute.getText(ast))
            ) {
              violations.push(`${relative}: ${node.tagName.getText(ast)} overrides its text role`)
            }
          }
        }
        ts.forEachChild(node, visit)
      }
      visit(ast)
    }
    expect(violations).toEqual([])
  })
})
