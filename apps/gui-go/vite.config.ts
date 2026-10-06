import { fileURLToPath } from 'node:url'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
const path = (relative: string) => fileURLToPath(new URL(relative, import.meta.url))
export default defineConfig({
  root: path('./frontend'),
  plugins: [react()],
  resolve: {
    alias: [
      { find: '@/lib/ipc', replacement: path('./frontend/src/native.ts') },
      { find: '@', replacement: path('../gui/src') },
      { find: /^pino$/, replacement: 'pino/browser' },
    ],
  },
  build: { target: 'safari15.6' },
})
