import { resolve } from 'path'
import { sentryVitePlugin } from '@sentry/vite-plugin'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

const host = process.env.TAURI_DEV_HOST
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
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
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
    exclude: ['**/node_modules/**', '**/dist/**', '**/.worktrees/**', '**/worktrees/**'],
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
