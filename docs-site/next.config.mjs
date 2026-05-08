import { createMDX } from 'fumadocs-mdx/next'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'

const __dirname = dirname(fileURLToPath(import.meta.url))

const withMDX = createMDX()

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // Pin Turbopack and the file-tracing root to docs-site so the monorepo
  // root's lockfile / package.json doesn't hijack module resolution.
  turbopack: {
    root: __dirname,
  },
  // `outputFileTracingRoot` is a third belt over `pnpm-workspace.yaml`
  // (find-root marker) and `turbopack.root` above. On Vercel + Next.js 16
  // it triggers a build-output mismatch — `.next/routes-manifest-deterministic.json`
  // ends up missing and the deploy step fails with ENOENT. Vercel's Root
  // Directory setting (`docs-site`) already isolates the build, so the
  // pin is unnecessary there.
  ...(process.env.VERCEL ? {} : { outputFileTracingRoot: __dirname }),
}

export default withMDX(config)
