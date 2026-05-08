import { createTokenizer } from '@orama/tokenizers/mandarin'
import { createFromSource } from 'fumadocs-core/search/server'
import { source } from '@/lib/source'

export const { GET } = createFromSource(source, {
  // Map fumadocs locale codes to Orama language settings.
  // Mandarin requires a dedicated tokenizer — `language: 'mandarin'` alone
  // does not split CJK text into searchable tokens.
  // https://docs.orama.com/docs/orama-js/supported-languages/using-chinese-with-orama
  localeMap: {
    en: { language: 'english' },
    zh: {
      components: {
        tokenizer: createTokenizer(),
      },
      search: {
        threshold: 0,
        tolerance: 0,
      },
    },
  },
})
