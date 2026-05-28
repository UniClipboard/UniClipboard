import defaultMdxComponents from 'fumadocs-ui/mdx'
import type { MDXComponents } from 'mdx/types'
import { APIPage } from '@/components/api-page'
import { Feature } from '@/components/feature'

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    APIPage,
    Feature,
    ...components,
  } satisfies MDXComponents
}

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>
}
