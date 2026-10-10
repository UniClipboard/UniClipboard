/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_SENTRY_DSN?: string
  readonly VITE_APP_VERSION?: string
  readonly VITE_APP_ENV?: string
  readonly VITE_E2E?: string
  readonly VITE_REACT_GRAB?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
