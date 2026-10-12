import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { sentryVitePlugin } from '@sentry/vite-plugin'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { createServer, type Plugin } from 'vite'
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
const uiPlatforms = ['windows', 'macos', 'linux']
const uiPlatformMarker = '.ui-platform'
// Main-window pages built on `@/ui-kit` replace the existing pages only for the targets listed here.
// A build without UC_UI_PLATFORM (vitest, coverage) keeps the existing pages and never reaches the
// kit; every shipped target (Windows, macOS, Linux) sets one.
const kitPages: Record<string, { platforms: string[]; kit: string; existing: string }> = {
  '@ui-history-page': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/history-kit/HistoryKitPage.tsx',
    existing: './src/pages/HistoryPage.tsx',
  },
  // The entry pages shown before the main window content (design page "Setup · Startup · Unlock").
  // The startup screen is also the first frame (src/startup-screen.tsx).
  '@ui-startup-screen': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/entry-kit/StartupKitScreen.tsx',
    existing: './src/components/app/StartupProgressScreen.tsx',
  },
  '@ui-app-status-screen': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/entry-kit/StatusKitScreen.tsx',
    existing: './src/components/app/AppStatusScreen.tsx',
  },
  '@ui-profile-recovery-page': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/entry-kit/ProfileRecoveryKitPage.tsx',
    existing: './src/pages/ProfileRecoveryPage.tsx',
  },
  '@ui-setup-page': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/entry-kit/SetupKitPage.tsx',
    existing: './src/pages/SetupPage.tsx',
  },
  '@ui-unlock-page': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/entry-kit/UnlockKitPage.tsx',
    existing: './src/pages/UnlockPage.tsx',
  },
  // Settings is presented by each platform skin.
  '@ui-settings-page': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/pages/settings-kit/SettingsKitPage.tsx',
    existing: './src/pages/SettingsPage.tsx',
  },
  // Group, row and the simple controls of the settings sections (the sections' logic is shared).
  '@ui-setting-parts': {
    platforms: ['macos', 'windows', 'linux'],
    kit: './src/components/setting/setting-parts/kit.tsx',
    existing: './src/components/setting/setting-parts/legacy.tsx',
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
      // `@ui-skin/first-frame` is the skin's subset for the first frame (src/ui-kit/first-frame.ts).
      if (source !== '@ui-skin' && source !== '@ui-skin/first-frame') return null
      if (!platform || !uiPlatforms.includes(platform)) {
        this.error(`@/ui-kit needs ${required} (got ${platform ? `"${platform}"` : 'nothing'})`)
      }
      const file = source === '@ui-skin' ? 'index.ts' : 'first-frame.ts'
      const entry = path(`./src/ui-kit/skins/${platform}/${file}`)
      if (!existsSync(entry)) this.error(`The ${platform} skin of @/ui-kit is missing: ${entry}`)
      return entry
    },
    generateBundle() {
      if (platform)
        this.emitFile({ type: 'asset', fileName: uiPlatformMarker, source: `${platform}\n` })
    },
  }
}

// The stylesheets a skin inlines into the prerendered first frame (Windows styles it with Griffel).
const skinCssFiles: Record<string, string[]> = {
  macos: ['tokens.css', 'macos.css', 'entry.css'],
  linux: ['tokens.css', 'adwaita.css', 'entry.css'],
}

// Prerender the pending view with the existing first-frame skin. This runs in Node, never
// ships react-dom/server or Griffel's renderer in the browser.
function startupSplash(): Plugin {
  const platform = process.env.UC_UI_PLATFORM
  let rendered:
    | Promise<{ markup: string; styles: string; exportCopy: Record<string, string> }>
    | undefined
  return {
    name: 'uc-startup-splash',
    transformIndexHtml: {
      order: 'pre',
      async handler(html, context) {
        if (
          !platform ||
          !uiPlatforms.includes(platform) ||
          !context.filename.endsWith('/index.html')
        )
          return html
        const patterns = {
          copy: /(<script type="application\/json" id="uc-splash-copy">)([\s\S]*?)(<\/script>)/,
          style: /<style>[\s\S]*?<\/style>/,
          root: /<div id="root">[\s\S]*?(?=\s*<script type="application\/json")/,
        }
        for (const [name, pattern] of Object.entries(patterns)) {
          if (!pattern.test(html)) throw new Error(`uc-startup-splash: missing ${name} HTML anchor`)
        }
        rendered ??= (async () => {
          const server = await createServer({
            configFile: false,
            root: path('.'),
            plugins: [uiSkin(), react()],
            resolve: { alias: { '@': path('./src') } },
            server: { middlewareMode: true, hmr: false },
            appType: 'custom',
            optimizeDeps: { noDiscovery: true, include: [] },
          })
          try {
            const { renderStartupSplash } = await server.ssrLoadModule(
              '/scripts/startup-splash.tsx'
            )
            return renderStartupSplash()
          } finally {
            await server.close()
          }
        })()
        const { markup, styles, exportCopy } = await rendered
        const skinCss = (skinCssFiles[platform] ?? [])
          .map(file => readFileSync(path(`./src/ui-kit/skins/${platform}/${file}`), 'utf8'))
          .join('\n')
        const frameCss = `
          .uc-splash { position:fixed; inset:0; }
          .uc-splash-frame { display:flex; height:100%; box-sizing:border-box; padding-top:2.5rem; }
          .uc-splash-wait { display:flex; flex-direction:column; gap:.5rem; }
          .uc-splash-caption { display:flex; justify-content:space-between; gap:.5rem; }
          html:not(.dark) [data-splash-theme=dark], html.dark [data-splash-theme=light] { display:none; }
        `
        if (process.env.VITE_GUI_GO_E2E === '1') {
          const observer = readFileSync(path('./e2e/startup-frame-observer.js'), 'utf8')
          html = html.replace('</head>', `<script>${observer}</script></head>`)
        }
        return html
          .replace(patterns.copy, (_, start, json, end) => {
            const copy = JSON.parse(json)
            for (const language of Object.keys(copy)) copy[language].export = exportCopy[language]
            return `${start}${JSON.stringify(copy)}${end}`
          })
          .replace(patterns.style, `${styles}<style>${skinCss}\n${frameCss}</style>`)
          .replace(patterns.root, `<div id="root">${markup}</div>\n`)
      },
    },
  }
}

// The whole frontend lives in this directory: the business sources are `src` (`@`), the Wails
// host modules `src/host`. The same config builds the three documents and runs the vitest suite.
export default defineConfig({
  plugins: [
    uiSkin(),
    startupSplash(),
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
