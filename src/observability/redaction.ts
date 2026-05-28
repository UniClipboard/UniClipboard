const sensitiveKeyPattern = /password|passphrase|pass[12]|secret|token|auth|api_?key/

export function redactSensitiveArgs(value: unknown): unknown {
  return redactValue(value, new WeakMap())
}

function redactValue(value: unknown, seen: WeakMap<object, unknown>): unknown {
  if (Array.isArray(value)) {
    if (seen.has(value)) {
      return seen.get(value)
    }

    const result: unknown[] = []
    seen.set(value, result)
    for (const item of value) {
      result.push(redactValue(item, seen))
    }
    return result
  }

  if (!value || typeof value !== 'object') {
    return value
  }

  if (!isPlainObject(value)) {
    return value
  }

  if (seen.has(value)) {
    return seen.get(value)
  }

  const result: Record<string, unknown> = {}
  seen.set(value, result)
  for (const [key, item] of Object.entries(value)) {
    const lowerKey = key.toLowerCase()
    if (sensitiveKeyPattern.test(lowerKey)) {
      result[key] = '[REDACTED]'
    } else {
      result[key] = redactValue(item, seen)
    }
  }

  return result
}

function isPlainObject(value: object): value is Record<string, unknown> {
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}
