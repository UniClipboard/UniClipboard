import { describe, expect, it } from 'vitest'
import {
  MAX_SERIALIZED_STACK_LENGTH,
  replaceErrorsDeep,
  serializeError,
  serializeUnknown,
} from '@/observability/serialize-error'

function withCause(message: string, cause: unknown): Error {
  return Object.assign(new Error(message), { cause })
}

describe('serializeError', () => {
  it('captures name, message and stack', () => {
    const err = new TypeError('boom')
    expect(serializeError(err)).toMatchObject({
      name: 'TypeError',
      message: 'boom',
      stack: expect.stringContaining('boom'),
    })
  })

  it('serializes nested causes, including non-Error causes', () => {
    const root = withCause('root', 'plain string cause')
    const top = withCause('top', root)

    const out = serializeError(top) as { cause: { message: string; cause: unknown } }

    expect(out.cause.message).toBe('root')
    expect(out.cause.cause).toBe('plain string cause')
  })

  it('does not loop on cyclic causes', () => {
    const a = new Error('a')
    const b = withCause('b', a)
    ;(a as Error & { cause?: unknown }).cause = b

    expect(() => JSON.stringify(serializeError(a))).not.toThrow()
    expect(JSON.stringify(serializeError(a))).toContain('[Circular]')
  })

  it('bounds the stack length', () => {
    const err = new Error('long')
    err.stack = 'x'.repeat(MAX_SERIALIZED_STACK_LENGTH * 3)

    const out = serializeError(err) as { stack: string }

    expect(out.stack.length).toBeLessThan(MAX_SERIALIZED_STACK_LENGTH + 64)
    expect(out.stack).toContain('[truncated]')
  })
})

describe('serializeUnknown', () => {
  it('keeps strings and truncates oversized ones', () => {
    expect(serializeUnknown('nope')).toBe('nope')
    expect(String(serializeUnknown('y'.repeat(100_000))).length).toBeLessThan(10_000)
  })

  it('serializes objects without throwing on cycles', () => {
    const obj: Record<string, unknown> = { code: 'E1' }
    obj.self = obj

    const out = serializeUnknown(obj)

    expect(() => JSON.stringify(out)).not.toThrow()
    expect(out).toMatchObject({ code: 'E1', self: '[Circular]' })
  })

  it('stringifies primitives and nullish values', () => {
    expect(serializeUnknown(undefined)).toBe('undefined')
    expect(serializeUnknown(null)).toBe('null')
    expect(serializeUnknown(42)).toBe(42)
  })
})

describe('replaceErrorsDeep', () => {
  it('replaces nested Error instances and leaves other values untouched', () => {
    const when = new Date(0)
    const out = replaceErrorsDeep({ err: new Error('x'), when, list: [new Error('y')] }) as {
      err: { message: string }
      when: Date
      list: Array<{ message: string }>
    }

    expect(out.err.message).toBe('x')
    expect(out.list[0].message).toBe('y')
    expect(out.when).toBe(when)
  })
})
