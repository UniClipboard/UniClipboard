import { createMDX } from 'fumadocs-mdx/next';
import { fileURLToPath } from 'node:url';
import { dirname } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // Pin Turbopack and the file-tracing root to docs-site so the monorepo
  // root's lockfile / package.json doesn't hijack module resolution.
  turbopack: {
    root: __dirname,
  },
  outputFileTracingRoot: __dirname,
};

export default withMDX(config);
