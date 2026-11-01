import { createHash } from 'node:crypto'
import ts from 'typescript'

export function inventory(source, name) {
  const file = ts.createSourceFile(name, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS)
  const assertions = []
  const scenarioSignals = []
  function visit(node) {
    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      node.expression.expression.getText(file) === 'assert'
    ) {
      assertions.push(node.getText(file))
    }
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      ['test', 'step', 'check', 'flow'].includes(node.expression.text) &&
      node.arguments[0] &&
      ts.isStringLiteralLike(node.arguments[0])
    ) {
      scenarioSignals.push(node.arguments[0].text)
    }
    ts.forEachChild(node, visit)
  }
  visit(file)
  return { sha256: createHash('sha256').update(source).digest('hex'), assertions, scenarioSignals }
}
