import { resolve } from 'path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// The shipped frontend is built by apps/gui-go/vite.config.ts (the Wails host). This
// config only serves the vitest suite of the shared sources in `src`.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      // Generated Wails bindings of the host commands (scripts/gen-host-bindings.mjs).
      '@host': resolve(
        '../gui-go/frontend/bindings/github.com/UniClipboard/UniClipboard/apps/gui-go'
      ),
      '@': resolve('./src'),
      // Use the browser-specific pino build, as the shipped bundle does
      pino: 'pino/browser',
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
