import { RootProvider } from 'fumadocs-ui/provider/next'
import '../global.css'
import { Manrope } from 'next/font/google'
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
        <RootProvider i18n={i18nUI.provider(lang)}>{children}</RootProvider>
      </body>
    </html>
  )
}
