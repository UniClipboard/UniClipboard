import '@testing-library/jest-dom/vitest'
import { vi } from 'vitest'

vi.stubEnv('VITE_SENTRY_DSN', 'https://example.com/1')
vi.stubEnv('VITE_APP_VERSION', 'test')

if (
  typeof globalThis.localStorage === 'undefined' ||
  typeof globalThis.localStorage.getItem !== 'function'
) {
  const store = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value)
      },
      removeItem: (key: string) => {
        store.delete(key)
      },
      clear: () => {
        store.clear()
      },
    },
    configurable: true,
  })
}

// jsdom ships no `matchMedia`; theme-aware code (sonner's Toaster, next-themes,
// `useThemeSync`) reads it on mount. Provide an inert stub so those components
// render in tests instead of throwing.
if (typeof globalThis.matchMedia !== 'function') {
  Object.defineProperty(globalThis, 'matchMedia', {
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
    configurable: true,
    writable: true,
  })
}

// jsdom ships no `ResizeObserver`; Base UI's ScrollArea observes its viewport in
// a layout effect. Provide an inert stub so components built on it render in
// tests instead of throwing.
if (typeof globalThis.ResizeObserver !== 'function') {
  Object.defineProperty(globalThis, 'ResizeObserver', {
    value: class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
    configurable: true,
    writable: true,
  })
}

// Base UI waits for viewport animations before measuring its scrollbar thumb.
// jsdom has no Web Animations API, so expose the no-animation result used by
// static test layouts.
if (typeof Element !== 'undefined' && typeof Element.prototype.getAnimations !== 'function') {
  Object.defineProperty(Element.prototype, 'getAnimations', {
    value: () => [],
    configurable: true,
    writable: true,
  })
}

await import('@/i18n')
