/**
 * Tauri event mocking utilities for Vitest.
 *
 * Problem: vi.mock() creates a private closure over its factory function,
 * so a `registry` Map declared inside the mock is NOT accessible from
 * outside. The solution is to expose the registry as a named export from
 * the mock so tests can both register listeners AND emit events via the
 * same shared Map.
 *
 * Usage:
 *   import { emitTauriEvent } from './_tauri-event-helpers'
 *   // in a test:
 *   emitTauriEvent('daemon://connection-info', { baseUrl: '...', token: '...' })
 */

import { vi } from 'vitest'

/** The shared registry — MUST be referenced from BOTH the mock and emitTauriEvent. */
export const _tauriEventRegistry: Map<string, (payload: unknown) => void> = new Map()

vi.mock('@tauri-apps/api/event', () => {
  return {
    listen: vi.fn((eventName: string, handler: (event: { payload: unknown }) => void) => {
      _tauriEventRegistry.set(eventName, handler as (payload: unknown) => void)
      return Promise.resolve(() => _tauriEventRegistry.delete(eventName))
    }),
  }
})

/**
 * Emit a Tauri event by name, invoking all registered handlers.
 * Uses the SAME _tauriEventRegistry that vi.mock('@tauri-apps/api/event') populates.
 */
export function emitTauriEvent<T>(eventName: string, payload: T): void {
  const handler = _tauriEventRegistry.get(eventName)
  if (handler) {
    handler(payload)
    _tauriEventRegistry.delete(eventName)
  }
}
