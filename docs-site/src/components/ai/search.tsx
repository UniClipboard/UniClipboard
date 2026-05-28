'use client'

import { useChat, type UseChatHelpers } from '@ai-sdk/react'
import { Presence } from '@radix-ui/react-presence'
import { DefaultChatTransport, type UIToolInvocation } from 'ai'
import { buttonVariants } from 'fumadocs-ui/components/ui/button'
import { Loader2, MessageCircleIcon, RefreshCw, SearchIcon, Send, X } from 'lucide-react'
import {
  type ComponentProps,
  createContext,
  type ReactNode,
  type SyntheticEvent,
  use,
  useEffect,
  useEffectEvent,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import type { ChatUIMessage, SearchTool } from '@/app/api/chat/route'
import { Markdown } from '@/components/markdown'
import { cn } from '@/lib/cn'
import { docsBasePath } from '@/lib/shared'

type AILocale = 'en' | 'zh'

const labels = {
  en: {
    title: 'AI Chat',
    notice: 'AI can be inaccurate. Please verify important answers.',
    close: 'Close',
    retry: 'Retry',
    clear: 'Clear Chat',
    answering: 'AI is answering...',
    question: 'Ask a question',
    abort: 'Abort Answer',
    userRole: 'you',
    assistantRole: 'UniClipboard',
    searchFailed: 'Failed to search',
    searching: 'Searching...',
    searchResults: (count: number) => `${count} search results`,
    empty: 'Start a new chat below.',
    requestFailed: 'Request Failed',
  },
  zh: {
    title: 'AI 对话',
    notice: 'AI 可能不准确，重要答案请核对文档。',
    close: '关闭',
    retry: '重试',
    clear: '清空对话',
    answering: 'AI 正在回答...',
    question: '输入问题',
    abort: '停止回答',
    userRole: '你',
    assistantRole: 'UniClipboard',
    searchFailed: '搜索失败',
    searching: '正在搜索...',
    searchResults: (count: number) => `${count} 条搜索结果`,
    empty: '在下方开始新对话。',
    requestFailed: '请求失败',
  },
}

const Context = createContext<{
  open: boolean
  setOpen: (open: boolean) => void
  chat: UseChatHelpers<ChatUIMessage>
  label: (typeof labels)[AILocale]
} | null>(null)

function AISearchPanelHeader({ className, ...props }: ComponentProps<'div'>) {
  const { label, setOpen } = useAISearchContext()

  return (
    <div
      className={cn(
        'sticky top-0 flex items-start gap-2 border rounded-xl bg-fd-secondary text-fd-secondary-foreground shadow-sm',
        className
      )}
      {...props}
    >
      <div className="px-3 py-2 flex-1">
        <p className="text-sm font-medium mb-2">{label.title}</p>
        <p className="text-xs text-fd-muted-foreground">{label.notice}</p>
      </div>

      <button
        type="button"
        aria-label={label.close}
        tabIndex={-1}
        className={cn(
          buttonVariants({
            size: 'icon-sm',
            color: 'ghost',
            className: 'text-fd-muted-foreground rounded-full',
          })
        )}
        onClick={() => setOpen(false)}
      >
        <X />
      </button>
    </div>
  )
}

function AISearchInputActions() {
  const { label } = useAISearchContext()
  const { messages, status, setMessages, regenerate } = useChatContext()
  const isLoading = status === 'streaming'

  if (messages.length === 0) return null

  return (
    <>
      {!isLoading && messages.at(-1)?.role === 'assistant' && (
        <button
          type="button"
          className={cn(
            buttonVariants({
              color: 'secondary',
              size: 'sm',
              className: 'rounded-full gap-1.5',
            })
          )}
          onClick={() => regenerate()}
        >
          <RefreshCw className="size-4" />
          {label.retry}
        </button>
      )}
      <button
        type="button"
        className={cn(
          buttonVariants({
            color: 'secondary',
            size: 'sm',
            className: 'rounded-full',
          })
        )}
        onClick={() => setMessages([])}
      >
        {label.clear}
      </button>
    </>
  )
}

const storageKeyInput = '__ai_search_input'

const subscribeNoop = () => () => {}
const getStoredInput = () => {
  try {
    return localStorage.getItem(storageKeyInput) ?? ''
  } catch {
    return ''
  }
}
const getStoredInputServer = () => ''

function AISearchInput(props: ComponentProps<'form'>) {
  const { label } = useAISearchContext()
  const { status, sendMessage, stop } = useChatContext()
  // Read from localStorage via useSyncExternalStore so SSR returns '' and the
  // client hydrates with the stored value in a single pass — no flicker.
  const storedInput = useSyncExternalStore(subscribeNoop, getStoredInput, getStoredInputServer)
  const [edited, setEdited] = useState<string | null>(null)
  const input = edited ?? storedInput
  const setInput = (v: string) => setEdited(v)
  const isLoading = status === 'streaming' || status === 'submitted'
  const onStart = (e?: SyntheticEvent) => {
    e?.preventDefault()
    const message = input.trim()
    if (message.length === 0) return

    void sendMessage({
      role: 'user',
      parts: [
        {
          type: 'data-client',
          data: {
            location: window.location.href,
          },
        },
        {
          type: 'text',
          text: message,
        },
      ],
    })
    setInput('')
    localStorage.removeItem(storageKeyInput)
  }

  useEffect(() => {
    if (isLoading) document.getElementById('nd-ai-input')?.focus()
  }, [isLoading])

  return (
    <form {...props} className={cn('flex items-start pe-2', props.className)} onSubmit={onStart}>
      <Input
        value={input}
        placeholder={isLoading ? label.answering : label.question}
        autoFocus
        className="p-3"
        disabled={status === 'streaming' || status === 'submitted'}
        onChange={e => {
          setInput(e.target.value)
          if (e.target.value.length === 0) localStorage.removeItem(storageKeyInput)
          else localStorage.setItem(storageKeyInput, e.target.value)
        }}
        onKeyDown={event => {
          if (!event.shiftKey && event.key === 'Enter') {
            onStart(event)
          }
        }}
      />
      {isLoading ? (
        <button
          key="bn"
          type="button"
          className={cn(
            buttonVariants({
              color: 'secondary',
              className: 'transition-all rounded-full mt-2 gap-2',
            })
          )}
          onClick={stop}
        >
          <Loader2 className="size-4 animate-spin text-fd-muted-foreground" />
          {label.abort}
        </button>
      ) : (
        <button
          key="bn"
          type="submit"
          className={cn(
            buttonVariants({
              color: 'primary',
              className: 'transition-all rounded-full mt-2',
            })
          )}
          disabled={input.length === 0}
        >
          <Send className="size-4" />
        </button>
      )}
    </form>
  )
}

function List(props: Omit<ComponentProps<'div'>, 'dir'>) {
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!containerRef.current) return
    function callback() {
      const container = containerRef.current
      if (!container) return

      container.scrollTo({
        top: container.scrollHeight,
        behavior: 'instant',
      })
    }

    const observer = new ResizeObserver(callback)
    callback()

    const element = containerRef.current?.firstElementChild

    if (element) {
      observer.observe(element)
    }

    return () => {
      observer.disconnect()
    }
  }, [])

  return (
    <div
      ref={containerRef}
      {...props}
      className={cn('fd-scroll-container overflow-y-auto min-w-0 flex flex-col', props.className)}
    >
      {props.children}
    </div>
  )
}

function Input(props: ComponentProps<'textarea'>) {
  const ref = useRef<HTMLDivElement>(null)
  const shared = cn('col-start-1 row-start-1', props.className)

  return (
    <div className="grid flex-1">
      <textarea
        id="nd-ai-input"
        {...props}
        className={cn(
          'resize-none bg-transparent placeholder:text-fd-muted-foreground focus-visible:outline-none',
          shared
        )}
      />
      <div ref={ref} className={cn(shared, 'break-all invisible')}>
        {`${props.value?.toString() ?? ''}\n`}
      </div>
    </div>
  )
}

function Message({ message, ...props }: { message: ChatUIMessage } & ComponentProps<'div'>) {
  const { label } = useAISearchContext()
  let markdown = ''
  const searchCalls: UIToolInvocation<SearchTool>[] = []

  for (const part of message.parts ?? []) {
    if (part.type === 'text') {
      markdown += part.text
      continue
    }

    if (part.type === 'tool-search') {
      searchCalls.push(part)
    }
  }

  return (
    <div onClick={e => e.stopPropagation()} {...props}>
      <p
        className={cn(
          'mb-1 text-sm font-medium text-fd-muted-foreground',
          message.role === 'assistant' && 'text-fd-primary'
        )}
      >
        {message.role === 'user' ? label.userRole : label.assistantRole}
      </p>
      <div className="prose text-sm">
        <Markdown text={markdown} />
      </div>

      {searchCalls.map(call => {
        return (
          <div
            key={call.toolCallId}
            className="flex flex-row gap-2 items-center mt-3 rounded-lg border bg-fd-secondary text-fd-muted-foreground text-xs p-2"
          >
            <SearchIcon className="size-4" />
            {call.state === 'output-error' || call.state === 'output-denied' ? (
              <p className="text-fd-error">{call.errorText ?? label.searchFailed}</p>
            ) : (
              <p>{!call.output ? label.searching : label.searchResults(call.output.length)}</p>
            )}
          </div>
        )
      })}
    </div>
  )
}

export function AISearch({ children, locale = 'en' }: { children: ReactNode; locale?: AILocale }) {
  const [open, setOpen] = useState(false)
  const chat = useChat<ChatUIMessage>({
    id: 'search',
    transport: new DefaultChatTransport({
      api: `${docsBasePath}/api/chat`,
    }),
  })

  return (
    <Context
      value={useMemo(
        () => ({
          chat,
          label: labels[locale],
          open,
          setOpen,
        }),
        [chat, locale, open]
      )}
    >
      {children}
    </Context>
  )
}

export function AISearchTrigger({
  position = 'default',
  className,
  ...props
}: ComponentProps<'button'> & { position?: 'default' | 'float' }) {
  const { open, setOpen } = useAISearchContext()

  return (
    <button
      type="button"
      data-state={open ? 'open' : 'closed'}
      className={cn(
        position === 'float' && [
          'fixed bottom-4 gap-3 w-24 inset-e-[calc(--spacing(4)+var(--removed-body-scroll-bar-size,0px))] shadow-lg z-20 transition-[translate,opacity]',
          open && 'translate-y-10 opacity-0',
        ],
        className
      )}
      onClick={() => setOpen(!open)}
      {...props}
    >
      {props.children}
    </button>
  )
}

export function AISearchPanel() {
  const { open, setOpen } = useAISearchContext()
  useHotKey()

  return (
    <>
      <style>
        {`
        @keyframes ask-ai-open {
          from {
            translate: 100% 0;
          }
          to {
            translate: 0 0;
          }
        }
        @keyframes ask-ai-close {
          from {
            width: var(--ai-chat-width);
          }
          to {
            width: 0px;
          }
        }`}
      </style>
      <Presence present={open}>
        <button
          type="button"
          aria-label="Close AI search"
          className={cn(
            'fixed inset-0 z-30 backdrop-blur-xs bg-fd-overlay lg:hidden cursor-default border-0 p-0',
            open ? 'animate-fd-fade-in' : 'animate-fd-fade-out'
          )}
          onClick={() => setOpen(false)}
        />
      </Presence>
      <Presence present={open}>
        <div
          className={cn(
            'overflow-hidden z-30 bg-fd-card text-fd-card-foreground [--ai-chat-width:400px] 2xl:[--ai-chat-width:460px]',
            'max-lg:fixed max-lg:inset-x-2 max-lg:inset-y-4 max-lg:border max-lg:rounded-2xl max-lg:shadow-xl',
            'lg:sticky lg:top-0 lg:h-dvh lg:border-s lg:ms-auto lg:in-[#nd-docs-layout]:[grid-area:toc] lg:in-[#nd-notebook-layout]:row-span-full lg:in-[#nd-notebook-layout]:col-start-5',
            open
              ? 'animate-fd-dialog-in lg:animate-[ask-ai-open_200ms]'
              : 'animate-fd-dialog-out lg:animate-[ask-ai-close_200ms]'
          )}
        >
          <div className="flex flex-col size-full p-2 lg:p-3 lg:w-(--ai-chat-width)">
            <AISearchPanelHeader />
            <AISearchPanelList className="flex-1" />
            <div className="rounded-xl border bg-fd-secondary text-fd-secondary-foreground shadow-sm has-focus-visible:shadow-md">
              <AISearchInput />
              <div className="flex items-center gap-1.5 p-1 empty:hidden">
                <AISearchInputActions />
              </div>
            </div>
          </div>
        </div>
      </Presence>
    </>
  )
}

function AISearchPanelList({ className, style, ...props }: ComponentProps<'div'>) {
  const { label } = useAISearchContext()
  const chat = useChatContext()
  const messages = chat.messages.filter(msg => msg.role !== 'system')

  return (
    <List
      className={cn('py-4 overscroll-contain', className)}
      style={{
        maskImage:
          'linear-gradient(to bottom, transparent, white 1rem, white calc(100% - 1rem), transparent 100%)',
        ...style,
      }}
      {...props}
    >
      {messages.length === 0 ? (
        <div className="text-sm text-fd-muted-foreground/80 size-full flex flex-col items-center justify-center text-center gap-2">
          <MessageCircleIcon fill="currentColor" stroke="none" />
          <p>{label.empty}</p>
        </div>
      ) : (
        <div className="flex flex-col px-3 gap-4">
          {chat.error && (
            <div className="p-2 bg-fd-secondary text-fd-secondary-foreground border rounded-lg">
              <p className="text-xs text-fd-muted-foreground mb-1">
                {label.requestFailed}: {chat.error.name}
              </p>
              <p className="text-sm">{chat.error.message}</p>
            </div>
          )}
          {messages.map(item => (
            <Message key={item.id} message={item} />
          ))}
        </div>
      )}
    </List>
  )
}

function useHotKey() {
  const { open, setOpen } = useAISearchContext()

  const onKeyPress = useEffectEvent((e: KeyboardEvent) => {
    if (e.key === 'Escape' && open) {
      setOpen(false)
      e.preventDefault()
    }

    if (e.key === '/' && (e.metaKey || e.ctrlKey) && !open) {
      setOpen(true)
      e.preventDefault()
    }
  })

  useEffect(() => {
    window.addEventListener('keydown', onKeyPress)
    return () => window.removeEventListener('keydown', onKeyPress)
  }, [])
}

export function useAISearchContext() {
  return use(Context)!
}

function useChatContext() {
  return use(Context)!.chat
}
