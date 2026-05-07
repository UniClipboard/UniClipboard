import { createFromSource } from 'fumadocs-core/search/server'
import { source } from '@/lib/source'

export const { GET } = createFromSource(source, {
  // Map fumadocs locale codes to Orama language settings.
  // https://docs.orama.com/docs/orama-js/supported-languages
  localeMap: {
    en: { language: 'english' },
    zh: { language: 'mandarin' },
  },
})
