import { buttonVariants } from 'fumadocs-ui/components/ui/button'
import { DocsLayout } from 'fumadocs-ui/layouts/docs'
import { MessageCircleIcon } from 'lucide-react'
import { AISearch, AISearchPanel, AISearchTrigger } from '@/components/ai/search'
import { cn } from '@/lib/cn'
import { baseOptions } from '@/lib/layout.shared'
import { source } from '@/lib/source'

export default async function Layout({ params, children }: LayoutProps<'/[lang]'>) {
  const { lang } = await params
  const locale = lang === 'zh' ? 'zh' : 'en'

  return (
    <DocsLayout tree={source.getPageTree(lang)} {...baseOptions(lang)}>
      <AISearch locale={locale}>
        <AISearchPanel />
        <AISearchTrigger
          position="float"
          className={cn(
            buttonVariants({
              color: 'secondary',
              className: 'text-fd-muted-foreground rounded-2xl',
            })
          )}
        >
          <MessageCircleIcon className="size-4" />
          {locale === 'zh' ? '问 AI' : 'Ask AI'}
        </AISearchTrigger>
      </AISearch>
      {children}
    </DocsLayout>
  )
}
