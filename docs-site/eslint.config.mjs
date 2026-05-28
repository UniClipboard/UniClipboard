import { defineConfig, globalIgnores } from 'eslint/config'
import nextVitals from 'eslint-config-next/core-web-vitals'

const eslintConfig = defineConfig([
  ...nextVitals,
  // Pin React version to skip eslint-plugin-react@7.37.5 auto-detect, which
  // calls the removed `context.getFilename()` API and crashes on ESLint 10.
  // Remove once upstream ships a fix (>7.37.5) compatible with ESLint 10.
  {
    settings: {
      react: { version: '19.2' },
    },
  },
  globalIgnores(['.next/**', 'out/**', 'build/**', 'next-env.d.ts', '.source/**']),
])

export default eslintConfig
