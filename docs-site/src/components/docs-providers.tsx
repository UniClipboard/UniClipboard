'use client'

import { RootProvider } from 'fumadocs-ui/provider/next'
import type { ReactNode } from 'react'
import { docsBasePath } from '@/lib/shared'

// fetch() in the search client doesn't honor Next.js `basePath`, so we must
// pin the API URL to the prefixed route registered by `app/api/search/route.ts`.
const SEARCH_API = `${docsBasePath}/api/search`

type I18nProp = Parameters<typeof RootProvider>[0]['i18n']

export function DocsProviders({ i18n, children }: { i18n: I18nProp; children: ReactNode }) {
  return (
    <RootProvider i18n={i18n} search={{ options: { api: SEARCH_API } }}>
      {children}
    </RootProvider>
  )
}
