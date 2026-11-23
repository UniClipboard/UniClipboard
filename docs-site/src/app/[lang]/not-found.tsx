'use client'

import Link from 'next/link'
import { useParams } from 'next/navigation'

const copy = {
  en: {
    code: '404',
    title: 'Page not found',
    body: "The page you're looking for doesn't exist or has moved.",
    home: 'Back to docs',
  },
  zh: {
    code: '404',
    title: '页面未找到',
    body: '你访问的页面不存在，或者已经被移动到别处。',
    home: '返回文档首页',
  },
} as const

type Lang = keyof typeof copy

export default function NotFound() {
  const params = useParams()
  const lang = (params?.lang as Lang) in copy ? (params.lang as Lang) : 'en'
  const t = copy[lang]
  const homeHref = lang === 'en' ? '/' : `/${lang}`

  return (
    <main className="flex flex-col items-center justify-center gap-3 py-24 text-center">
      <p className="text-sm font-medium text-fd-muted-foreground">{t.code}</p>
      <h1 className="text-2xl font-semibold">{t.title}</h1>
      <p className="max-w-md text-fd-muted-foreground">{t.body}</p>
      <Link
        href={homeHref}
        className="mt-4 inline-flex items-center rounded-md bg-fd-primary px-4 py-2 text-sm font-medium text-fd-primary-foreground hover:opacity-90"
      >
        {t.home}
      </Link>
    </main>
  )
}
