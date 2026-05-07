import Link from 'next/link'
import { i18n } from '@/lib/i18n'

const copy = {
  en: {
    title: 'UniClipboard Docs',
    lead: 'A cross-device clipboard, built for keyboard-driven flow.',
    cta: 'Read the docs',
  },
  zh: {
    title: 'UniClipboard 文档',
    lead: '为键盘流而生的跨设备剪贴板。',
    cta: '阅读文档',
  },
} as const

type Lang = (typeof i18n.languages)[number]

export default async function HomePage(props: PageProps<'/[lang]'>) {
  const { lang } = await props.params
  const t = copy[lang as Lang] ?? copy.en
  const docsHref = lang === i18n.defaultLanguage ? '/docs' : `/${lang}/docs`

  return (
    <main className="flex flex-1 flex-col items-center justify-center px-6 py-24 text-center">
      <h1 className="text-4xl font-bold tracking-tight md:text-5xl">{t.title}</h1>
      <p className="mt-4 max-w-xl text-lg text-fd-muted-foreground">{t.lead}</p>
      <Link
        href={docsHref}
        className="mt-8 inline-flex items-center rounded-md bg-fd-primary px-5 py-2.5 text-sm font-medium text-fd-primary-foreground transition hover:opacity-90"
      >
        {t.cta} →
      </Link>
    </main>
  )
}
