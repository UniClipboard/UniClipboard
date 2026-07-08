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

await import('@/i18n')
