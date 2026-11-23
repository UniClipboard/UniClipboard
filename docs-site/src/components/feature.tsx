'use client'

import { AlertTriangle, Flag, FlaskConical } from 'lucide-react'
import { useParams } from 'next/navigation'
import { type ReactNode } from 'react'
import { twMerge as cn } from 'tailwind-merge'

type Status = 'stable' | 'experimental' | 'deprecated'

type Props = {
  /** Version this feature is available from, e.g. "0.7.0" (no leading "v"). */
  since: string
  /** Defaults to `stable`. `experimental` and `deprecated` get colored pills. */
  status?: Status
  /** Override the link the version chip points to. Defaults to the GitHub release tag. */
  releaseUrl?: string
  /** Only meaningful for `status="deprecated"`. */
  replacedBy?: { label: string; href: string }
  className?: string
  children?: ReactNode
}

const RELEASE_BASE = 'https://github.com/UniClipboard/UniClipboard/releases/tag/v'

const COPY = {
  en: {
    available: 'Available since',
    experimental: 'experimental',
    deprecated: 'deprecated',
    replacedBy: 'replaced by',
  },
  zh: {
    available: '此版本起可用',
    experimental: '实验性',
    deprecated: '已弃用',
    replacedBy: '替代为',
  },
} as const

export function Feature({
  since,
  status = 'stable',
  releaseUrl,
  replacedBy,
  className,
  children,
}: Props) {
  const params = useParams<{ lang?: string }>()
  const lang: 'en' | 'zh' = params?.lang === 'zh' ? 'zh' : 'en'
  const t = COPY[lang]

  const version = since.startsWith('v') ? since.slice(1) : since
  const href = releaseUrl ?? `${RELEASE_BASE}${version}`

  const tone =
    status === 'deprecated'
      ? 'border-red-500/30 bg-red-500/5'
      : status === 'experimental'
        ? 'border-amber-500/40 bg-amber-500/5'
        : 'border-fd-border bg-fd-muted/40'

  const Icon =
    status === 'deprecated' ? AlertTriangle : status === 'experimental' ? FlaskConical : Flag

  const iconTone =
    status === 'deprecated'
      ? 'text-red-600 dark:text-red-400'
      : status === 'experimental'
        ? 'text-amber-600 dark:text-amber-400'
        : 'text-fd-muted-foreground'

  return (
    <aside
      role="note"
      className={cn(
        'my-4 flex gap-3 rounded-lg border px-4 py-3 text-sm [&_p]:my-0',
        tone,
        className
      )}
    >
      <Icon className={cn('mt-0.5 size-4 shrink-0', iconTone)} aria-hidden />
      <div className="flex-1 space-y-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-fd-foreground">
          <span className="font-medium">
            {t.available}{' '}
            <a
              href={href}
              target="_blank"
              rel="noreferrer noopener"
              className="font-mono underline decoration-fd-muted-foreground/40 underline-offset-4 hover:text-fd-primary hover:decoration-fd-primary"
            >
              v{version}
            </a>
          </span>
          {status !== 'stable' && <span className="text-fd-muted-foreground">·</span>}
          {status === 'experimental' && (
            <span className="rounded-full border border-amber-500/40 px-2 py-0.5 text-xs font-medium text-amber-700 dark:text-amber-300">
              {t.experimental}
            </span>
          )}
          {status === 'deprecated' && (
            <span className="rounded-full border border-red-500/40 px-2 py-0.5 text-xs font-medium text-red-600 dark:text-red-300">
              {t.deprecated}
            </span>
          )}
          {status === 'deprecated' && replacedBy && (
            <span className="text-fd-muted-foreground">
              · {t.replacedBy}{' '}
              <a
                href={replacedBy.href}
                className="text-fd-foreground underline decoration-fd-muted-foreground/40 underline-offset-4 hover:decoration-fd-primary"
              >
                {replacedBy.label}
              </a>
            </span>
          )}
        </div>
        {children && <div className="text-fd-muted-foreground">{children}</div>}
      </div>
    </aside>
  )
}
