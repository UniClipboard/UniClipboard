'use client'

import { buttonVariants } from 'fumadocs-ui/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from 'fumadocs-ui/components/ui/popover'
import { ChevronDown, ExternalLinkIcon, TextIcon } from 'lucide-react'
import { usePathname } from 'next/navigation'
import { useMemo, type ReactNode } from 'react'
import { twMerge as cn } from 'tailwind-merge'
import { docsBasePath } from '@/lib/shared'

type Props = {
  markdownUrl?: string
  githubUrl?: string
  className?: string
  children?: ReactNode
}

// Mirrors fumadocs' ViewOptionsPopover but builds the AI prompt URL
// from `docsBasePath + pathname` so the prompt sent to ChatGPT / Claude /
// Cursor / Scira resolves under the `/docs` basePath. fumadocs uses
// `usePathname()` directly, which strips basePath in Next.js.
export function PageViewOptions({ markdownUrl, githubUrl, className, children }: Props) {
  const pathname = usePathname()

  const items = useMemo(() => {
    const fullPath = `${docsBasePath}${pathname}`
    const target =
      typeof window === 'undefined'
        ? fullPath
        : new URL(fullPath, window.location.origin).toString()
    const q = `Read ${target}, I want to ask questions about it.`

    return [
      githubUrl && {
        title: 'Open in GitHub',
        href: githubUrl,
        icon: <GitHubIcon />,
      },
      markdownUrl && {
        title: 'View as Markdown',
        href: markdownUrl,
        icon: <TextIcon />,
      },
      {
        title: 'Open in Scira AI',
        href: `https://scira.ai/?${new URLSearchParams({ q })}`,
        icon: <SciraIcon />,
      },
      {
        title: 'Open in ChatGPT',
        href: `https://chatgpt.com/?${new URLSearchParams({ hints: 'search', q })}`,
        icon: <ChatGPTIcon />,
      },
      {
        title: 'Open in Claude',
        href: `https://claude.ai/new?${new URLSearchParams({ q })}`,
        icon: <ClaudeIcon />,
      },
      {
        title: 'Open in Cursor',
        href: `https://cursor.com/link/prompt?${new URLSearchParams({ text: q })}`,
        icon: <CursorIcon />,
      },
    ].filter(Boolean) as Array<{ title: string; href: string; icon: ReactNode }>
  }, [githubUrl, markdownUrl, pathname])

  return (
    <Popover>
      <PopoverTrigger
        className={cn(
          buttonVariants({ color: 'secondary', size: 'sm' }),
          'gap-2 data-[state=open]:bg-fd-accent data-[state=open]:text-fd-accent-foreground',
          className
        )}
      >
        {children ?? 'Open'}
        <ChevronDown className="size-3.5 text-fd-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="flex flex-col">
        {items.map(item => (
          <a
            key={item.href}
            href={item.href}
            rel="noreferrer noopener"
            target="_blank"
            className="text-sm p-2 rounded-lg inline-flex items-center gap-2 hover:text-fd-accent-foreground hover:bg-fd-accent [&_svg]:size-4"
          >
            {item.icon}
            {item.title}
            <ExternalLinkIcon className="text-fd-muted-foreground size-3.5 ms-auto" />
          </a>
        ))}
      </PopoverContent>
    </Popover>
  )
}

function GitHubIcon() {
  return (
    <svg fill="currentColor" role="img" viewBox="0 0 24 24">
      <title>GitHub</title>
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  )
}

function SciraIcon() {
  return (
    <svg viewBox="0 0 910 934" fill="none" xmlns="http://www.w3.org/2000/svg">
      <title>Scira AI</title>
      <path
        d="M647.664 197.775C569.13 189.049 525.5 145.419 516.774 66.88C508.048 145.419 464.418 189.049 385.884 197.775C464.418 206.501 508.048 250.131 516.774 328.665C525.5 250.131 569.13 206.501 647.664 197.775Z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="8"
        strokeLinejoin="round"
      />
      <path
        d="M516.774 304.217C510.299 275.491 498.208 252.087 480.335 234.214C462.462 216.341 439.058 204.251 410.333 197.775C439.059 191.3 462.462 179.209 480.335 161.336C498.208 143.463 510.299 120.06 516.774 91.334C523.25 120.059 535.34 143.463 553.213 161.336C571.086 179.209 594.49 191.3 623.216 197.775C594.49 204.251 571.086 216.341 553.213 234.214C535.34 252.087 523.25 275.491 516.774 304.217Z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="8"
        strokeLinejoin="round"
      />
      <path
        d="M857.5 508.116C763.259 497.644 710.903 445.288 700.432 351.047C689.961 445.288 637.605 497.644 543.364 508.116C637.605 518.587 689.961 570.943 700.432 665.184C710.903 570.943 763.259 518.587 857.5 508.116Z"
        stroke="currentColor"
        strokeWidth="20"
        strokeLinejoin="round"
      />
      <path
        d="M700.432 615.957C691.848 589.05 678.575 566.357 660.383 548.165C642.191 529.973 619.499 516.7 592.593 508.116C619.499 499.533 642.191 486.258 660.383 468.066C678.575 449.874 691.848 427.181 700.432 400.274C709.015 427.181 722.289 449.874 740.481 468.066C758.673 486.258 781.365 499.533 808.271 508.116C781.365 516.7 758.673 529.973 740.481 548.165C722.289 566.357 709.015 589.05 700.432 615.957Z"
        stroke="currentColor"
        strokeWidth="20"
        strokeLinejoin="round"
      />
      <path
        d="M889.949 121.237C831.049 114.692 798.326 81.96 791.782 23.06C785.237 81.96 752.515 114.692 693.614 121.237C752.515 127.781 785.237 160.504 791.782 219.404C798.326 160.504 831.049 127.781 889.949 121.237Z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="8"
        strokeLinejoin="round"
      />
      <path
        d="M791.782 196.795C786.697 176.937 777.869 160.567 765.16 147.858C752.452 135.15 736.082 126.322 716.226 121.237C736.082 116.152 752.452 107.324 765.16 94.61C777.869 81.90 786.697 65.53 791.782 45.67C796.867 65.53 805.695 81.90 818.403 94.61C831.112 107.324 847.481 116.152 867.338 121.237C847.481 126.322 831.112 135.15 818.403 147.858C805.694 160.567 796.867 176.937 791.782 196.795Z"
        fill="currentColor"
        stroke="currentColor"
        strokeWidth="8"
        strokeLinejoin="round"
      />
      <path
        d="M760.632 764.337C720.719 814.616 669.835 855.1 611.872 882.692C553.91 910.285 490.404 924.255 426.213 923.533C362.022 922.812 298.846 907.419 241.518 878.531C184.19 849.643 134.228 808.026 95.45 756.863C56.68 705.7 30.12 646.346 17.81 583.343C5.50 520.339 7.76 455.354 24.42 393.359C41.089 331.364 71.70 274.001 113.947 225.658C156.184 177.315 208.919 139.273 268.117 114.442"
        stroke="currentColor"
        strokeWidth="30"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function ChatGPTIcon() {
  return (
    <svg role="img" viewBox="0 0 24 24" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
      <title>OpenAI</title>
      <path d="M22.28 9.82a5.98 5.98 0 0 0-.5157-4.91 6.04 6.04 0 0 0-6.50-2.9A6.06 6.06 0 0 0 4.98 4.18a5.98 5.98 0 0 0-3.99 2.9 6.04 6.04 0 0 0 .7427 7.09 5.98 5.98 0 0 0 .511 4.91 6.051 6.051 0 0 0 6.51 2.90A5.98 5.98 0 0 0 13.25 24a6.05 6.05 0 0 0 5.77-4.20 5.98 5.98 0 0 0 3.99-2.90 6.05 6.05 0 0 0-.7475-7.07zm-9.022 12.60a4.47 4.47 0 0 1-2.87-1.04l.1419-.0804 4.77-2.75a.7948.79 0 0 0 .3927-.6813v-6.73l2.02 1.16a.071.071 0 0 1 .038.052v5.58a4.504 4.504 0 0 1-4.49 4.49zm-9.66-4.12a4.47 4.47 0 0 1-.5346-3.01l.142.08 4.783 2.75a.7712.77 0 0 0 .7806 0l5.84-3.36v2.33a.0804.08 0 0 1-.0332.06L9.74 19.95a4.49 4.49 0 0 1-6.14-1.64zM2.34 7.89a4.485 4.485 0 0 1 2.36-1.97V11.6a.7664.76 0 0 0 .3879.67l5.81 3.35-2.02 1.16a.0757.07 0 0 1-.071 0l-4.83-2.78A4.504 4.504 0 0 1 2.34 7.872zm16.59 3.85L13.10 8.364 15.11 7.2a.0757.07 0 0 1 .071 0l4.83 2.79a4.49 4.49 0 0 1-.6765 8.10v-5.67a.79.79 0 0 0-.407-.667zm2.01-3.02l-.142-.0852-4.77-2.78a.7759.77 0 0 0-.7854 0L9.409 9.22V6.89a.0662.06 0 0 1 .0284-.0615l4.83-2.78a4.49 4.49 0 0 1 6.68 4.66zM8.30 12.863l-2.02-1.16a.0804.08 0 0 1-.038-.0567V6.07a4.49 4.49 0 0 1 7.37-3.45l-.142.08L8.704 5.459a.7948.79 0 0 0-.3927.68zm1.09-2.36l2.602-1.49 2.60 1.49v2.99l-2.59 1.49-2.60-1.49Z" />
    </svg>
  )
}

function ClaudeIcon() {
  return (
    <svg fill="currentColor" role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <title>Anthropic</title>
      <path d="M17.30 3.541h-3.67l6.696 16.918H24Zm-10.60 0L0 20.459h3.74l1.36-3.55h7.00l1.36 3.55h3.74L10.53 3.54Zm-.3712 10.22 2.29-5.94 2.29 5.94Z" />
    </svg>
  )
}

function CursorIcon() {
  return (
    <svg fill="currentColor" role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
      <title>Cursor</title>
      <path d="M11.503.131 1.891 5.678a.84.84 0 0 0-.42.726v11.188c0 .3.162.575.42.724l9.609 5.55a1 1 0 0 0 .998 0l9.61-5.55a.84.84 0 0 0 .42-.724V6.404a.84.84 0 0 0-.42-.726L12.497.131a1.01 1.01 0 0 0-.996 0M2.657 6.338h18.55c.263 0 .43.287.297.515L12.23 22.918c-.062.107-.229.064-.229-.06V12.335a.59.59 0 0 0-.295-.51l-9.11-5.257c-.109-.063-.064-.23.061-.23" />
    </svg>
  )
}
