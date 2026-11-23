import defaultMdxComponents from 'fumadocs-ui/mdx'
import { Step, Steps } from 'fumadocs-ui/components/steps'
import { Tab, Tabs } from 'fumadocs-ui/components/tabs'
import type { MDXComponents } from 'mdx/types'
import { Feature } from '@/components/feature'
import { Mermaid } from '@/components/mermaid'

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    Feature,
    Step,
    Steps,
    Tab,
    Tabs,
    Mermaid,
    ...components,
  } satisfies MDXComponents
}

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>
}
