import '../global.css'
import { Manrope } from 'next/font/google'
import { DocsProviders } from '@/components/docs-providers'
import { i18nUI } from '@/lib/layout.shared'

const manrope = Manrope({
  subsets: ['latin'],
  variable: '--font-manrope',
})

export default async function Layout({ params, children }: LayoutProps<'/[lang]'>) {
  const { lang } = await params
  return (
    <html lang={lang} className={manrope.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <DocsProviders i18n={i18nUI.provider(lang)}>{children}</DocsProviders>
      </body>
    </html>
  )
}
