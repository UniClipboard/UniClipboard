import bindingsSource from '@host/hostservice.ts?raw'
import { setTransport } from '@wailsio/runtime'

// Browser fixtures run the real pages without a native host. The pages call the Wails-generated
// `HostService` bindings, which reach the host through the Wails runtime transport; this installs
// the runtime's own extension point (`setTransport`) so a fixture answers each binding call by its Go
// method name. Calls without a handler resolve to null and are recorded in `window.__ucNativeCalls`.
// Window, event and other runtime calls are accepted and ignored.
type Handler = (...args: unknown[]) => unknown

const CALL_OBJECT = 0
const CALL_BINDING = 0

const methodNames = new Map<number, string>(
  [
    ...bindingsSource.matchAll(
      /export function (\w+)\([^)]*\)[^{]*\{\s*return \$Call\.ByID\((\d+)/g
    ),
  ].map(match => [Number(match[2]), match[1]])
)

/** Rejects a binding call the way the Go host does: an error named RuntimeError whose `cause` is the marshaled error. */
export class HostRejection extends Error {
  constructor(readonly cause: unknown) {
    super('host rejection')
  }
}

export function installFakeHost(handlers: Record<string, Handler>) {
  const calls: { command: string; handled: boolean }[] = []
  Object.assign(window, { __UC_DESKTOP_HOST__: true, __ucNativeCalls: calls })
  setTransport({
    call: async (objectID: number, method: number, _windowName: string, args: unknown) => {
      if (objectID !== CALL_OBJECT || method !== CALL_BINDING) return null
      const request = args as { methodID: number; args?: unknown[] }
      const name = methodNames.get(request.methodID) ?? `#${request.methodID}`
      const handler = handlers[name]
      calls.push({ command: name, handled: Boolean(handler) })
      try {
        return handler ? await handler(...(request.args ?? [])) : null
      } catch (error) {
        if (!(error instanceof HostRejection)) throw error
        throw Object.assign(new Error('host rejection', { cause: error.cause }), {
          name: 'RuntimeError',
        })
      }
    },
  })
}
