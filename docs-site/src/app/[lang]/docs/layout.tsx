import { DocsLayout } from 'fumadocs-ui/layouts/docs'
import { baseOptions } from '@/lib/layout.shared'
import { source } from '@/lib/source'

export default async function Layout({ params, children }: LayoutProps<'/[lang]/docs'>) {
  const { lang } = await params
  return (
    <DocsLayout tree={source.getPageTree(lang)} {...baseOptions(lang)}>
      {children}
    </DocsLayout>
  )
}
