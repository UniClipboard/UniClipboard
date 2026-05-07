import { notFound } from 'next/navigation'
import { getLLMText, getPageMarkdownUrl, source } from '@/lib/source'

export const revalidate = false

export async function GET(
  _req: Request,
  { params }: RouteContext<'/llms.mdx/docs/[lang]/[[...slug]]'>
) {
  const { slug, lang } = await params
  const page = source.getPage(slug?.slice(0, -1), lang)
  if (!page) notFound()

  return new Response(await getLLMText(page), {
    headers: {
      'Content-Type': 'text/markdown',
    },
  })
}

export function generateStaticParams() {
  return source.getPages().map(page => ({
    lang: page.locale,
    slug: getPageMarkdownUrl(page).segments,
  }))
}
