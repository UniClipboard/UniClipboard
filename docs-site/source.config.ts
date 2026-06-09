import { remarkMdxMermaid } from 'fumadocs-core/mdx-plugins'
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema'
import { defineConfig, defineDocs } from 'fumadocs-mdx/config'
import { z } from 'zod'

// You can customize Zod schemas for frontmatter and `meta.json` here
// see https://fumadocs.dev/docs/mdx/collections
export const docs = defineDocs({
  dir: 'content/docs',
  docs: {
    schema: pageSchema.extend({
      metaTitle: z.string().optional(),
    }),
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
  meta: {
    schema: metaSchema,
  },
})

export default defineConfig({
  mdxOptions: {
    // Render ```mermaid code blocks as <Mermaid> diagrams. The component is
    // registered in src/components/mdx.tsx.
    remarkPlugins: [remarkMdxMermaid],
  },
})
