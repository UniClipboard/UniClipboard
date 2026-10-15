/**
 * Bounded, cycle-safe serialization of `Error` instances and arbitrary
 * rejection reasons for logging. `JSON.stringify(new Error('x'))` yields `{}`
 * because `name`/`message`/`stack` are non-enumerable, so log payloads must
 * pass through here first.
 */

export const MAX_SERIALIZED_STACK_LENGTH = 4000
export const MAX_SERIALIZED_STRING_LENGTH = 2000
const MAX_DEPTH = 5

export interface SerializedError {
  name: string
  message: string
  stack?: string
  cause?: unknown
}

function truncate(value: string, max: number): string {
  return value.length > max ? `${value.slice(0, max)}...[truncated]` : value
}

function isPlainObject(value: object): boolean {
  const proto = Object.getPrototypeOf(value)
  return proto === Object.prototype || proto === null
}

export function serializeError(
  error: Error,
  depth = 0,
  seen: WeakSet<object> = new WeakSet()
): SerializedError | string {
  if (seen.has(error)) return '[Circular]'
  seen.add(error)
  const out: SerializedError = {
    name: error.name,
    message: truncate(error.message, MAX_SERIALIZED_STRING_LENGTH),
  }
  if (error.stack) out.stack = truncate(error.stack, MAX_SERIALIZED_STACK_LENGTH)
  const cause = (error as Error & { cause?: unknown }).cause
  if (cause !== undefined) {
    out.cause = depth + 1 >= MAX_DEPTH ? '[MaxDepth]' : convert(cause, depth + 1, seen, true)
  }
  return out
}

function convert(value: unknown, depth: number, seen: WeakSet<object>, full: boolean): unknown {
  if (value instanceof Error) return serializeError(value, depth, seen)
  if (typeof value === 'string') return truncate(value, MAX_SERIALIZED_STRING_LENGTH)
  if (value === undefined || value === null) return String(value)
  if (typeof value !== 'object') {
    return typeof value === 'function' || typeof value === 'symbol' ? String(value) : value
  }
  const isArray = Array.isArray(value)
  if (!isArray && !isPlainObject(value)) {
    // Untouched by the Error-replacement pass; stringified for reason dumps.
    return full ? truncate(String(value), MAX_SERIALIZED_STRING_LENGTH) : value
  }
  if (seen.has(value)) return '[Circular]'
  if (depth >= MAX_DEPTH) return '[MaxDepth]'
  seen.add(value)
  const out: unknown = isArray
    ? value.map(item => convert(item, depth + 1, seen, full))
    : Object.fromEntries(
        Object.entries(value).map(([k, v]) => [k, convert(v, depth + 1, seen, full)])
      )
  seen.delete(value)
  return out
}

/** Serialize any rejection reason / thrown value into a JSON-safe, bounded form. */
export function serializeUnknown(value: unknown): unknown {
  return convert(value, 0, new WeakSet(), true)
}

/**
 * Replace nested `Error` instances inside plain objects/arrays with their
 * serialized form, leaving every other value (Date, class instances) as is.
 */
export function replaceErrorsDeep(value: unknown): unknown {
  return convert(value, 0, new WeakSet(), false)
}
