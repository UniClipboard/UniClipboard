import { defineConfig } from '@hey-api/openapi-ts'

export default defineConfig({
  // Local spec produced by the gen-openapi cargo bin (offline, reproducible).
  input: './schema/openapi.json',
  output: {
    path: 'src/api/generated',
    // 0.97.3 renamed `format`/`lint` -> `postProcess` (the old keys are
    // deprecated). Prettier's preset passes `--ignore-path ./.prettierignore`,
    // and `src/api/generated/` is listed there, so prettier no-ops on the tree
    // and the committed bytes == raw codegen output (the determinism the CI
    // drift-check relies on). Run it anyway so the post-process step is explicit
    // and matches the generator's own formatting.
    postProcess: ['prettier'],
  },
  plugins: [
    '@hey-api/client-fetch', // fetch runtime emitted into output/core + output/client
    '@hey-api/typescript',
    '@hey-api/sdk',
  ],
})
