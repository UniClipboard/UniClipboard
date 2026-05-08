import { docs } from 'collections/server'
import { loader } from 'fumadocs-core/source'
import { lucideIconsPlugin } from 'fumadocs-core/source/lucide-icons'
import { i18n } from './i18n'
import { docsBasePath, docsContentRoute, docsImageRoute, docsRoute } from './shared'

// See https://fumadocs.dev/docs/headless/source-api for more info
export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource(),
  plugins: [lucideIconsPlugin()],
  i18n,
})

export function getPageImage(page: (typeof source)['$inferPage']) {
  const segments = [...page.slugs, 'image.png']
  const locale = page.locale ?? i18n.defaultLanguage

  return {
    segments,
    url: `${docsBasePath}${docsImageRoute}/${locale}/${segments.join('/')}`,
  }
}

export function getPageMarkdownUrl(page: (typeof source)['$inferPage']) {
  const segments = [...page.slugs, 'content.md']
  const locale = page.locale ?? i18n.defaultLanguage

  return {
    segments,
    url: `${docsBasePath}${docsContentRoute}/${locale}/${segments.join('/')}`,
  }
}

export async function getLLMText(page: (typeof source)['$inferPage']) {
  const processed = await page.data.getText('processed')

  return `# ${page.data.title} (${page.url})

${processed}`
}
