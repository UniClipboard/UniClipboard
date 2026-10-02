import { defineConfig } from 'vitest/config'

// Root vitest only covers the repository tooling tests in scripts/__tests__.
// The GUI test suite has its own config in apps/gui/vite.config.ts; run it with
// `bun run test` so it executes inside apps/gui.
export default defineConfig({
  test: {
    include: ['scripts/__tests__/**/*.test.ts'],
  },
})
