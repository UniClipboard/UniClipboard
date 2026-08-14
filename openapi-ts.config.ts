import { defineConfig } from '@hey-api/openapi-ts'

export default defineConfig({
  // Local spec produced by the gen-openapi cargo bin (offline, reproducible).
  input: './schema/openapi.json',
  output: {
    path: 'src/api/generated',
  },
  plugins: [
    '@hey-api/client-fetch', // fetch runtime emitted into output/core + output/client
    '@hey-api/typescript',
    '@hey-api/sdk',
  ],
})
