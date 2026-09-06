import { AnimatePresence, m, useReducedMotion, type Transition } from 'framer-motion'
/*!
MIT License

Copyright (c) 2026 Saurabh Chauhan

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/
// Animated Toast Stack, adapted from https://beui.dev/r/animated-toast-stack/raw
import { AlertCircle, Bell, Check, Info, LoaderCircle, X, type LucideIcon } from 'lucide-react'
import { memo, type ReactNode, type Ref } from 'react'
import { EASE_OUT } from '@/lib/ease'
import { cn } from '@/lib/utils'
import type { AnimatedToast, ToastClassNames, ToastStatus } from './animated-toast-types'
const STACK_SPRING: Transition = {
  type: 'spring',
  stiffness: 420,
  damping: 34,
  mass: 0.75,
}

const CONTENT_TRANSITION = {
  duration: 0.28,
  ease: EASE_OUT,
} as const

const STATUS_ICON: Record<ToastStatus, LucideIcon> = {
  neutral: Bell,
  info: Info,
  loading: LoaderCircle,
  success: Check,
  error: AlertCircle,
}

const STATUS_CLASS: Record<ToastStatus, string> = {
  neutral: 'text-muted-foreground bg-primary/[0.05]',
  info: 'text-primary bg-primary/10',
  loading: 'text-primary bg-primary/10',
  success: 'text-emerald-600 bg-emerald-500/10 dark:text-emerald-400',
  error: 'text-destructive bg-destructive/10',
}

export const AnimatedToastItem = memo(function ToastItem({
  toast,
  index,
  onDismiss,
  classNames,
  icons,
  renderToast,
  dismissLabel,
  ref,
}: {
  toast: AnimatedToast
  index: number
  onDismiss?: (id: string) => void
  classNames?: ToastClassNames
  icons?: Partial<Record<ToastStatus, ReactNode>>
  renderToast?: (toast: AnimatedToast) => ReactNode
  dismissLabel: string
  ref?: Ref<HTMLLIElement>
}) {
  const reduce = useReducedMotion()
  const status = toast.status ?? 'neutral'
  const Icon = STATUS_ICON[status]
  const iconNode = icons?.[status] ?? toast.icon ?? <Icon className="size-3.5" />
  const canDismiss = toast.dismissible !== false && Boolean(onDismiss)

  return (
    <m.li
      ref={ref}
      data-toast-id={toast.id}
      data-status={status}
      layout
      initial={reduce ? { opacity: 0 } : { opacity: 0, y: 22, scale: 0.96, filter: 'blur(10px)' }}
      animate={reduce ? { opacity: 1 } : { opacity: 1, y: 0, scale: 1, filter: 'blur(0px)' }}
      exit={
        reduce
          ? { opacity: 0 }
          : {
              opacity: 0,
              x: 32,
              scale: 0.96,
              filter: 'blur(8px)',
              transition: { duration: 0.18, ease: EASE_OUT },
            }
      }
      transition={STACK_SPRING}
      drag={canDismiss && !reduce ? 'x' : false}
      dragConstraints={{ left: 0, right: 0 }}
      dragElastic={0.18}
      onDragEnd={(_, info) => {
        if (!canDismiss || !onDismiss) return
        if (Math.abs(info.offset.x) > 72 || Math.abs(info.velocity.x) > 520) {
          onDismiss(toast.id)
        }
      }}
      className={cn(
        'pointer-events-auto relative shrink-0 will-change-transform',
        classNames?.item
      )}
      style={{ zIndex: 20 - index }}
    >
      <div
        className={cn(
          'relative overflow-hidden rounded-2xl border border-border bg-card/95 p-3 text-card-foreground shadow-2xl backdrop-blur-xl',
          classNames?.surface
        )}
      >
        {renderToast ? (
          renderToast(toast)
        ) : (
          <div className="flex items-start gap-3">
            <m.span
              layout
              className={cn(
                'mt-0.5 inline-flex size-7 shrink-0 items-center justify-center rounded-full',
                STATUS_CLASS[status],
                classNames?.iconWrap
              )}
            >
              <AnimatePresence mode="popLayout" initial={false}>
                <m.span
                  key={status}
                  initial={
                    reduce ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.8, filter: 'blur(6px)' }
                  }
                  animate={
                    reduce ? { opacity: 1 } : { opacity: 1, y: 0, scale: 1, filter: 'blur(0px)' }
                  }
                  exit={
                    reduce ? { opacity: 0 } : { opacity: 0, y: -8, scale: 0.9, filter: 'blur(6px)' }
                  }
                  transition={CONTENT_TRANSITION}
                  className="inline-flex"
                >
                  {status === 'loading' ? (
                    <span className="inline-flex motion-safe:animate-spin">{iconNode}</span>
                  ) : (
                    iconNode
                  )}
                </m.span>
              </AnimatePresence>
            </m.span>

            <div className={cn('min-w-0 flex-1', classNames?.content)}>
              <AnimatePresence mode="popLayout" initial={false}>
                <m.div
                  key={`${toast.id}-${status}-${String(toast.title)}`}
                  initial={reduce ? { opacity: 0 } : { opacity: 0, y: 8, filter: 'blur(6px)' }}
                  animate={reduce ? { opacity: 1 } : { opacity: 1, y: 0, filter: 'blur(0px)' }}
                  exit={reduce ? { opacity: 0 } : { opacity: 0, y: -8, filter: 'blur(6px)' }}
                  transition={CONTENT_TRANSITION}
                >
                  <p
                    className={cn(
                      'break-words text-sm font-medium leading-5 text-foreground',
                      classNames?.title
                    )}
                  >
                    {toast.title}
                  </p>
                  {toast.description ? (
                    <p
                      className={cn(
                        'mt-0.5 break-words text-xs leading-4 text-muted-foreground',
                        classNames?.description
                      )}
                    >
                      {toast.description}
                    </p>
                  ) : null}
                </m.div>
              </AnimatePresence>

              {toast.action ? (
                <button
                  type="button"
                  onClick={() => toast.action?.onClick(toast)}
                  className={cn(
                    'mt-2 inline-flex h-7 items-center rounded-full bg-primary/[0.06] px-3 text-xs font-medium text-foreground transition-colors hover:bg-primary/[0.1]',
                    classNames?.action
                  )}
                >
                  {toast.action.label}
                </button>
              ) : null}
            </div>

            {canDismiss ? (
              <button
                type="button"
                onClick={() => onDismiss?.(toast.id)}
                aria-label={dismissLabel}
                className={cn(
                  'inline-flex size-7 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-primary/[0.06] hover:text-foreground',
                  classNames?.close
                )}
              >
                <X className="size-3.5" />
              </button>
            ) : null}
          </div>
        )}
      </div>
    </m.li>
  )
})
