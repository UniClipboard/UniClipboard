import type { MetadataRoute } from 'next'
import { i18n } from '@/lib/i18n'
import { docsBasePath, getSiteUrl } from '@/lib/shared'
import { source } from '@/lib/source'

export default function sitemap(): MetadataRoute.Sitemap {
  const origin = getSiteUrl()
  const langs = source.getLanguages()
  const enPages = langs.find(l => l.language === i18n.defaultLanguage)?.pages ?? []
  const slugToLangUrls = new Map<string, Record<string, string>>()

  for (const { language, pages } of langs) {
    for (const page of pages) {
      const key = page.slugs.join('/')
      const bucket = slugToLangUrls.get(key) ?? {}
      bucket[language] = `${origin}${docsBasePath}${page.url}`
      slugToLangUrls.set(key, bucket)
    }
  }

  return enPages.map(page => {
    const key = page.slugs.join('/')
    const langs = slugToLangUrls.get(key) ?? {}
    return {
      url: langs[i18n.defaultLanguage] ?? `${origin}${docsBasePath}${page.url}`,
      lastModified: new Date(),
      alternates: {
        languages: langs,
      },
    }
  })
}
