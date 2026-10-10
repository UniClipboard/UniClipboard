import { fileURLToPath } from 'node:url'
import { sentryVitePlugin } from '@sentry/vite-plugin'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

const path = (relative: string) => fileURLToPath(new URL(relative, import.meta.url))
const host = (name: string) => path(`./src/host/${name}.ts`)

// Release builds upload source maps to Sentry so production stack traces resolve to the original
// .tsx file and line. Without the token and project (local development, PR builds without secrets)
// no source map is emitted at all; with them the maps are deleted after the upload, so they never
// ship inside the app.
const sentryAuthToken = process.env.SENTRY_AUTH_TOKEN
const sentryProject = process.env.VITE_SENTRY_PROJECT
const sentryEnabled = Boolean(sentryAuthToken && sentryProject)
const appVersion = process.env.VITE_APP_VERSION

// The whole frontend lives in this directory: the business sources are `src` (`@`), the Wails
// host modules `src/host`. The same config builds the three documents and runs the vitest suite.
// Only the module boundary toward the native shell is replaced: each Tauri
// package id resolves to a Wails-backed adapter in `src/host`.
export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    sentryVitePlugin({
      org: process.env.SENTRY_ORG,
      project: sentryProject,
      authToken: sentryAuthToken,
      release: appVersion ? { name: appVersion } : undefined,
      sourcemaps: { filesToDeleteAfterUpload: ['**/*.map'] },
      disable: !sentryEnabled,
    }),
  ],
  optimizeDeps: { include: ['cuelume'] },
  resolve: {
    alias: [
      { find: '@tauri-apps/api/core', replacement: host('core') },
      { find: '@tauri-apps/api/event', replacement: host('event') },
      { find: '@tauri-apps/api/window', replacement: host('window') },
      { find: '@tauri-apps/api/webview', replacement: host('webview') },
      { find: '@tauri-apps/api/app', replacement: host('app') },
      { find: '@tauri-apps/plugin-opener', replacement: host('opener') },
      { find: '@tauri-apps/plugin-notification', replacement: host('notification') },
      // Generated Wails bindings of the host commands (scripts/gen-host-bindings.mjs).
      {
        find: '@host',
        replacement: path('./bindings/github.com/UniClipboard/UniClipboard/apps/gui-go'),
      },
      { find: '@', replacement: path('./src') },
      { find: /^pino$/, replacement: 'pino/browser' },
    ],
  },
  server: {
    // Wails proxies to this address over IPv4, so do not bind `localhost` (which may resolve to ::1).
    host: '127.0.0.1',
    port: Number(process.env.UC_DEV_SERVER_PORT ?? 1520),
    strictPort: true,
    fs: { allow: [path('../../..')] },
  },
  build: {
    target: 'safari15.6',
    cssTarget: 'safari15.6',
    emptyOutDir: true,
    sourcemap: sentryEnabled ? 'hidden' : false,
    // Three documents: main app, quick panel, updater.
    rollupOptions: {
      input: {
        main: path('./index.html'),
        'quick-panel': path('./quick-panel.html'),
        updater: path('./updater.html'),
      },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: './src/test/setup.ts',
    // docs-site/ 有独立的 CI job（docs-check.yml 的 test:config）与自己的
    // 工具链（node:test + 独立 vitest 环境）；根 vitest 的默认 include 会把
    // docs-site/test/next-config.test.mjs 卷进来，导致 "Cannot bundle Node.js
    // built-in node:test"（根环境无法 bundle node:test）。
    exclude: [
      '**/node_modules/**',
      '**/dist/**',
      '**/.worktrees/**',
      '**/worktrees/**',
      '**/docs-site/**',
    ],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      reportsDirectory: './coverage/frontend',
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/**/*.d.ts',
        'src/**/__tests__/**',
        'src/**/*.{test,spec}.{ts,tsx}',
        'src/test/**',
      ],
    },
  },
})
