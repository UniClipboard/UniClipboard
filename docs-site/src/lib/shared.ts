export const appName = 'UniClipboard'
export const docsRoute = '/'
export const docsImageRoute = '/og'
export const docsContentRoute = '/llms.mdx'
export const docsBasePath = '/docs'

export const gitConfig = {
  user: 'UniClipboard',
  repo: 'UniClipboard',
  branch: 'main',
}

export function getSiteUrl(): string {
  if (process.env.NEXT_PUBLIC_SITE_URL) return process.env.NEXT_PUBLIC_SITE_URL.replace(/\/$/, '')
  if (process.env.VERCEL_PROJECT_PRODUCTION_URL)
    return `https://${process.env.VERCEL_PROJECT_PRODUCTION_URL}`
  if (process.env.VERCEL_URL) return `https://${process.env.VERCEL_URL}`
  return 'http://localhost:3000'
}
