import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { includeIgnoreFile } from '@eslint/compat'
import js from '@eslint/js'
import prettier from 'eslint-config-prettier'
import { createTypeScriptImportResolver } from 'eslint-import-resolver-typescript'
import importX from 'eslint-plugin-import-x'
import reactX from 'eslint-plugin-react'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import globals from 'globals'
import tseslint from 'typescript-eslint'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const gitignorePath = path.join(__dirname, '.gitignore')

export default tseslint.config(
  includeIgnoreFile(gitignorePath),
  {
    ignores: [
      '.claude',
      'dist',
      'node_modules',
      'src-tauri/target',
      // tauri-specta codegen artifact (issue #698) — schema 由 Rust 端
      // 用 cargo test --test specta_export 重新生成；不应被 eslint 校验。
      'src/lib/ipc-bindings.generated.ts',
      // 同一 specta export test 生成的错误分级表（USER_FACING_ERROR_CODES）。
      'src/lib/error-severity.generated.ts',
      // @hey-api/openapi-ts 生成的 daemon HTTP 客户端（ADR-008 P5）。由
      // `bun run gen:api` 从 schema/openapi.json 重新生成；committed 的内容必须
      // 与生成器输出逐字节一致（CI 用 git diff --exit-code 做 drift-check）。
      // 因此排除在 eslint 之外，避免 lint-staged 的 `eslint --fix` 改写它。
      'src/api/generated/',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    ...reactX.configs.flat.recommended,
    ...reactX.configs.flat['jsx-runtime'],
  },
  reactHooks.configs.flat.recommended,
  {
    plugins: {
      'react-refresh': reactRefresh,
    },
    rules: {
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
      'react/prop-types': 'off',
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/no-explicit-any': 'warn',
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'off',
      'react-hooks/set-state-in-effect': 'off',
    },
  },
  {
    plugins: {
      'import-x': importX,
    },
    settings: {
      // 让 @/* 别名被解析成 src/* 真实路径,从而归入 import-x/order 的 internal 组。
      // 没有这个 resolver 时,@/* 会被当成 unknown,与 AGENTS.md 文档约定的顺序不一致。
      'import-x/resolver-next': [createTypeScriptImportResolver({ project: './tsconfig.json' })],
    },
    rules: {
      'import-x/order': [
        'error',
        {
          groups: ['builtin', 'external', 'internal', 'parent', 'sibling', 'index'],
          'newlines-between': 'never',
          alphabetize: { order: 'asc', caseInsensitive: true },
        },
      ],
    },
  },
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.es2021,
      },
    },
  },
  {
    files: ['e2e/**/*.js', 'e2e/**/*.mjs'],
    languageOptions: {
      globals: {
        ...globals.mocha,
        ...globals.node,
      },
    },
  },
  prettier
)
