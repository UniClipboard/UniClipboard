import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

const path = (relative: string) => fileURLToPath(new URL(relative, import.meta.url))
const host = (name: string) => path(`./frontend/src/host/${name}.ts`)

// The business frontend is `apps/gui/src`, referenced in place through `@`.
// Only the module boundary toward the native shell is replaced: each Tauri
// package id resolves to a Wails-backed adapter in `frontend/src/host`.
export default defineConfig({
  root: path('./frontend'),
  publicDir: path('../gui/public'),
  plugins: [react(), tailwindcss()],
  optimizeDeps: { include: ['cuelume'] },
  resolve: {
    alias: [
      { find: '@tauri-apps/api/core', replacement: host('core') },
      { find: '@tauri-apps/api/event', replacement: host('event') },
      { find: '@tauri-apps/api/window', replacement: host('window') },
      { find: '@tauri-apps/api/webview', replacement: host('webview') },
      { find: '@tauri-apps/api/app', replacement: host('app') },
      { find: '@tauri-apps/plugin-opener', replacement: host('opener') },
      { find: '@tauri-apps/plugin-log', replacement: host('log') },
      { find: '@tauri-apps/plugin-notification', replacement: host('notification') },
      { find: '@', replacement: path('../gui/src') },
      { find: /^pino$/, replacement: 'pino/browser' },
    ],
  },
  server: {
    // Wails proxies to this address over IPv4, so do not bind `localhost` (which may resolve to ::1).
    host: '127.0.0.1',
    port: Number(process.env.UC_DEV_SERVER_PORT ?? 1520),
    strictPort: true,
    fs: { allow: [path('../..')] },
  },
  build: {
    target: 'safari15.6',
    cssTarget: 'safari15.6',
    emptyOutDir: true,
    // Same three documents as the Tauri build: main app, quick panel, updater.
    rollupOptions: {
      input: {
        main: path('./frontend/index.html'),
        'quick-panel': path('./frontend/quick-panel.html'),
        updater: path('./frontend/updater.html'),
      },
    },
  },
})
