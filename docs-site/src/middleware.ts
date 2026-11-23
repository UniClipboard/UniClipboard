import { createI18nMiddleware } from 'fumadocs-core/i18n/middleware'
import { isMarkdownPreferred, rewritePath } from 'fumadocs-core/negotiation'
import { NextFetchEvent, NextRequest, NextResponse } from 'next/server'
import { i18n } from '@/lib/i18n'
import { docsContentRoute } from '@/lib/shared'

const i18nMiddleware = createI18nMiddleware(i18n)

const localeRewrites = i18n.languages.map(lang => {
  const prefix = lang === i18n.defaultLanguage ? '' : `/${lang}`
  return {
    suffix: rewritePath(`${prefix}{/*path}.mdx`, `${docsContentRoute}/${lang}{/*path}/content.md`)
      .rewrite,
    md: rewritePath(`${prefix}{/*path}`, `${docsContentRoute}/${lang}{/*path}/content.md`).rewrite,
  }
})

export default function proxy(request: NextRequest, event: NextFetchEvent) {
  const pathname = request.nextUrl.pathname

  // Root `/` — rewrite to default-locale home, no trailing slash.
  // (fumadocs' default-locale middleware would rewrite `/` -> `/en/`,
  //  which Next.js 308-redirects to `/en`, then back to `/` -> 404 loop.)
  if (pathname === '/') {
    const url = request.nextUrl.clone()
    url.pathname = `/${i18n.defaultLanguage}`
    return NextResponse.rewrite(url)
  }

  // Markdown extension rewrite — first match wins.
  for (const r of localeRewrites) {
    const result = r.suffix(pathname)
    if (result) {
      return NextResponse.rewrite(new URL(result, request.nextUrl))
    }
  }

  if (isMarkdownPreferred(request)) {
    for (const r of localeRewrites) {
      const result = r.md(pathname)
      if (result) {
        return NextResponse.rewrite(new URL(result, request.nextUrl))
      }
    }
  }

  return i18nMiddleware(request, event)
}

export const config = {
  // Match every request except Next.js internals, API routes, and files
  // with extensions. API routes (e.g. `/api/search`) must not run through
  // the i18n middleware — it would try to redirect them under a locale.
  // Listing root `/` separately because the negative-lookahead pattern below
  // doesn't reliably match an empty path with path-to-regexp.
  matcher: ['/', '/((?!_next|api/|favicon.ico|.*\\..*).*)'],
}
