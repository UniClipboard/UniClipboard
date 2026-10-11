import { existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { sentryVitePlugin } from '@sentry/vite-plugin'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

const path = (relative: string) => fileURLToPath(new URL(relative, import.meta.url))

// Release builds upload source maps to Sentry so production stack traces resolve to the original
// .tsx file and line. Without the token and project (local development, PR builds without secrets)
// no source map is emitted at all; with them the maps are deleted after the upload, so they never
// ship inside the app.
const sentryAuthToken = process.env.SENTRY_AUTH_TOKEN
const sentryProject = process.env.VITE_SENTRY_PROJECT
const sentryEnabled = Boolean(sentryAuthToken && sentryProject)
const appVersion = process.env.VITE_APP_VERSION
if (sentryEnabled && !appVersion) {
  throw new Error('VITE_APP_VERSION is required when uploading Sentry source maps')
}

// The platform UI kit (`src/ui-kit/contract.ts`) has one skin per target platform. The target is
// an explicit build input, never guessed from the host OS. Only bundles that import the kit need
// it, so a missing value is an error when `@ui-skin` is resolved; a value that is not a known
// platform is an error for every build. A build with a target writes it to `.ui-platform` in the
// output, and the packaging scripts refuse a dist built for another target.
const uiPlatforms = ['windows', 'macos']
const uiPlatformMarker = '.ui-platform'
// Main-window pages built on `@/ui-kit` replace the existing pages only for the targets listed here.
// Every other build (Windows, and Linux, which has no skin and no target) keeps the existing page,
// so a build without UC_UI_PLATFORM never reaches the kit.
const kitPages: Record<string, { platforms: string[]; kit: string; existing: string }> = {
  '@ui-history-page': {
    platforms: ['macos', 'windows'],
    kit: './src/pages/history-kit/HistoryKitPage.tsx',
    existing: './src/pages/HistoryPage.tsx',
  },
}
function uiSkin(): Plugin {
  const platform = process.env.UC_UI_PLATFORM
  const required = `UC_UI_PLATFORM=${uiPlatforms.join('|')}`
  return {
    name: 'uc-ui-skin',
    enforce: 'pre',
    buildStart() {
      if (platform && !uiPlatforms.includes(platform)) this.error(`${required} (got "${platform}")`)
    },
    resolveId(source) {
      const page = kitPages[source]
      if (page) {
        return path(platform && page.platforms.includes(platform) ? page.kit : page.existing)
      }
      if (source !== '@ui-skin') return null
      if (!platform || !uiPlatforms.includes(platform)) {
        this.error(`@/ui-kit needs ${required} (got ${platform ? `"${platform}"` : 'nothing'})`)
      }
      const entry = path(`./src/ui-kit/skins/${platform}/index.ts`)
      if (!existsSync(entry)) this.error(`The ${platform} skin of @/ui-kit is missing: ${entry}`)
      return entry
    },
    generateBundle() {
      if (platform)
        this.emitFile({ type: 'asset', fileName: uiPlatformMarker, source: `${platform}\n` })
    },
  }
}

// The whole frontend lives in this directory: the business sources are `src` (`@`), the Wails
// host modules `src/host`. The same config builds the three documents and runs the vitest suite.
export default defineConfig({
  plugins: [
    uiSkin(),
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
