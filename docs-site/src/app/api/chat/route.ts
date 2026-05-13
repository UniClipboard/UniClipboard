import { createAnthropic } from '@ai-sdk/anthropic'
import { createOpenAI } from '@ai-sdk/openai'
import {
  convertToModelMessages,
  stepCountIs,
  streamText,
  tool,
  type InferUITool,
  type UIMessage,
} from 'ai'
import { z } from 'zod'
import { docsSearchServer } from '@/lib/docs-search'
import { docsBasePath } from '@/lib/shared'

type DocsLocale = 'en' | 'zh'
type AIProvider = 'openai' | 'anthropic'

function getAIProvider(): AIProvider {
  const provider = process.env.AI_PROVIDER ?? 'openai'
  if (provider === 'openai' || provider === 'anthropic') return provider

  throw new Error(`Unsupported AI_PROVIDER: ${provider}`)
}

function requireEnv(name: string) {
  const value = process.env[name]?.trim()
  if (!value) {
    throw new Error(`${name} is not configured.`)
  }

  return value
}

function optionalEnv(name: string) {
  const value = process.env[name]?.trim()
  return value ? value : undefined
}

function createChatModel() {
  const provider = getAIProvider()

  if (provider === 'anthropic') {
    const anthropic = createAnthropic({
      apiKey: requireEnv('ANTHROPIC_API_KEY'),
      baseURL: optionalEnv('ANTHROPIC_BASE_URL'),
      name: optionalEnv('ANTHROPIC_PROVIDER_NAME'),
    })

    return anthropic.chat(requireEnv('ANTHROPIC_MODEL'))
  }

  const openai = createOpenAI({
    apiKey: requireEnv('OPENAI_API_KEY'),
    baseURL: optionalEnv('OPENAI_BASE_URL'),
    name: optionalEnv('OPENAI_PROVIDER_NAME') ?? 'openai-compatible',
  })

  return openai.chat(requireEnv('OPENAI_MODEL'))
}

function withDocsBasePath(url: string) {
  if (url === '/') return docsBasePath
  return `${docsBasePath}${url}`
}

function getClientLocale(messages: ChatUIMessage[]): DocsLocale {
  for (const message of messages.toReversed()) {
    for (const part of message.parts ?? []) {
      if (part.type !== 'data-client') continue

      try {
        const pathname = new URL(part.data.location).pathname
        if (pathname === `${docsBasePath}/zh` || pathname.startsWith(`${docsBasePath}/zh/`)) {
          return 'zh'
        }
      } catch {
        continue
      }
    }
  }

  return 'en'
}

function createSearchTool(defaultLocale: DocsLocale) {
  return tool({
    description: 'Search UniClipboard documentation and return matching sections.',
    inputSchema: z.object({
      query: z.string().min(1),
      locale: z.enum(['en', 'zh']).optional(),
      limit: z.number().int().min(1).max(20).default(8),
    }),
    async execute({ query, limit, locale }) {
      const results = await docsSearchServer.search(query, {
        limit,
        locale: locale ?? defaultLocale,
      })
      return results.map(result => ({
        ...result,
        url: withDocsBasePath(result.url),
      }))
    },
  })
}

export type SearchTool = ReturnType<typeof createSearchTool>

export type ChatUIMessage = UIMessage<
  never,
  {
    client: {
      location: string
    }
  },
  {
    search: InferUITool<SearchTool>
  }
>

const systemPrompt = [
  '你是 UniClipboard 文档站的 AI 助手。',
  '需要文档依据时，先调用 `search` 工具检索相关文档。',
  '根据用户语言回答：中文问题用中文回答，英文问题用英文回答。',
  '使用搜索结果作为依据，并用结果里的 `url` 字段添加 Markdown 链接引用。',
  '如果搜索结果无法支持答案，明确说明不知道，并建议更具体的查询。',
].join('\n')

export async function POST(req: Request) {
  const reqJson = await req.json()
  const messages = Array.isArray(reqJson.messages) ? reqJson.messages : []
  const searchTool = createSearchTool(getClientLocale(messages))

  let model
  try {
    model = createChatModel()
  } catch (error) {
    return new Response(error instanceof Error ? error.message : 'AI provider is not configured.', {
      status: 503,
    })
  }

  const result = streamText({
    model,
    system: systemPrompt,
    stopWhen: stepCountIs(5),
    tools: {
      search: searchTool,
    },
    messages: [
      ...(await convertToModelMessages<ChatUIMessage>(messages, {
        convertDataPart(part) {
          if (part.type === 'data-client')
            return {
              type: 'text',
              text: `[Client Context: ${JSON.stringify(part.data)}]`,
            }
        },
      })),
    ],
    toolChoice: 'auto',
  })

  return result.toUIMessageStreamResponse()
}
