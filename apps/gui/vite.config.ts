import { resolve } from 'path'
import { sentryVitePlugin } from '@sentry/vite-plugin'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

const host = process.env.TAURI_DEV_HOST
const devServerPort = Number(process.env.UC_DEV_SERVER_PORT ?? 1420)
const hmrPort = process.env.UC_DEV_SERVER_PORT ? devServerPort : 1421
const sentryAuthToken = process.env.SENTRY_AUTH_TOKEN
const sentryOrg = process.env.SENTRY_ORG
const sentryProject = process.env.VITE_SENTRY_PROJECT
const appVersion = process.env.VITE_APP_VERSION

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    // Upload sourcemaps to Sentry during release builds so production stack
    // traces resolve back to original .tsx file/line. Disabled when
    // SENTRY_AUTH_TOKEN is missing (local dev, PR builds without secrets).
    sentryVitePlugin({
      org: sentryOrg,
      project: sentryProject,
      authToken: sentryAuthToken,
      release: appVersion ? { name: appVersion } : undefined,
      disable: !sentryAuthToken || !sentryProject,
    }),
  ],

  // Multi-page build: main app + clipboard panel
  build: {
    target: 'safari15.6',
    cssTarget: 'safari15.6',
    // 'hidden' generates sourcemaps for upload but strips the
    // //# sourceMappingURL= comment from emitted JS, so the public bundle
    // does not advertise the map location.
    sourcemap: 'hidden',
    rollupOptions: {
      input: {
        main: resolve('./index.html'),
        'quick-panel': resolve('./quick-panel.html'),
        updater: resolve('./updater.html'),
      },
    },
  },

  // Keep cuelume in the startup optimization set so a cache created from a
  // different source state cannot trigger a mid-session dependency re-bundle.
  optimizeDeps: {
    include: ['cuelume'],
  },

  // Configure source aliases.
  resolve: {
    alias: {
      '@': resolve('./src'),
      // Use the browser-specific pino build in the WebView bundle
      pino: 'pino/browser',
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects the fixed devUrl port; stale listeners from crashed dev
  //    sessions are swept by `scripts/sweep-dev-port.mjs` (beforeDevCommand)
  //    instead of auto-incrementing, which Tauri cannot follow.
  server: {
    port: devServerPort,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: hmrPort,
        }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
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
