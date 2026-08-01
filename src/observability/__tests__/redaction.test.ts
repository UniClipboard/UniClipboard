import { describe, expect, it } from 'vitest'
import { redactSensitiveArgs } from '../redaction'

describe('redactSensitiveArgs', () => {
  it('masks sensitive keys recursively', () => {
    const input = {
      password: 'secret',
      pass1: 'alpha',
      pass2: 'beta',
      nested: { passphrase: 'hello' },
      list: [{ token: 'abc' }],
      safe: 'ok',
    }

    const output = redactSensitiveArgs(input)

    expect(output).toEqual({
      password: '[REDACTED]',
      pass1: '[REDACTED]',
      pass2: '[REDACTED]',
      nested: { passphrase: '[REDACTED]' },
      list: [{ token: '[REDACTED]' }],
      safe: 'ok',
    })
  })

  it('masks file paths passed to native commands', () => {
    expect(
      redactSensitiveArgs({
        arg0: {
          filePaths: ['/Users/mark/Documents/private.txt'],
        },
      })
    ).toEqual({
      arg0: {
        filePaths: '[REDACTED]',
      },
    })
  })

  it('passes through primitive and null values', () => {
    expect(redactSensitiveArgs(null)).toBeNull()
    expect(redactSensitiveArgs(undefined)).toBeUndefined()
    expect(redactSensitiveArgs('ok')).toBe('ok')
    expect(redactSensitiveArgs(42)).toBe(42)
    expect(redactSensitiveArgs(true)).toBe(true)
  })

  it('does not treat non-plain objects as records', () => {
    const input = new Date('2025-01-01T00:00:00Z')

    const output = redactSensitiveArgs(input)

    expect(output).toBe(input)
  })

  it('handles circular references safely', () => {
    const input: { password: string; self?: unknown } = {
      password: 'secret',
    }
    input.self = input

    const output = redactSensitiveArgs(input) as { password: string; self?: unknown }

    expect(output).not.toBe(input)
    expect(output.password).toBe('[REDACTED]')
    expect(output.self).toBe(output)
  })
})
