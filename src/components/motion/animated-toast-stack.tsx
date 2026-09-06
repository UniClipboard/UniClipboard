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
import { AnimatePresence } from 'framer-motion'
import { createPortal } from 'react-dom'
import { cn } from '@/lib/utils'
import { AnimatedToastItem } from './animated-toast-item'
import type { AnimatedToastStackProps, ToastPosition } from './animated-toast-types'
const POSITION_CLASS: Record<ToastPosition, string> = {
  'top-left': 'left-4 top-4',
  'top-center': 'left-1/2 top-4 -translate-x-1/2',
  'top-right': 'right-4 top-4',
  'bottom-left': 'bottom-6 left-4',
  'bottom-center': 'bottom-6 left-1/2 -translate-x-1/2',
  'bottom-right': 'bottom-6 right-4',
}

export function AnimatedToastStack({
  toasts,
  onDismiss,
  position = 'bottom-right',
  placement,
  fixed = false,
  portal,
  portalRoot,
  maxVisible = 4,
  className,
  classNames,
  icons,
  renderToast,
  label = 'Notifications',
  dismissLabel = 'Dismiss toast',
}: AnimatedToastStackProps) {
  const visibleToasts = toasts.slice(-maxVisible)
  const isBottom = position.startsWith('bottom')
  const resolvedPlacement = placement ?? (fixed ? 'fixed' : 'static')
  const shouldPortal = portal ?? resolvedPlacement === 'fixed'

  // Keep the live region mounted in body so modal isolation does not hide it.
  const portalTarget = shouldPortal ? (portalRoot ?? document.body) : null

  const stack = (
    <ol
      data-animated-toast-stack=""
      aria-label={label}
      aria-live="polite"
      aria-atomic="false"
      className={cn(
        'pointer-events-none flex w-[calc(100vw-2rem)] max-w-sm max-h-[calc(100dvh-3rem)] overflow-y-auto overscroll-contain gap-2',
        isBottom ? 'flex-col-reverse' : 'flex-col',
        resolvedPlacement === 'fixed' && 'fixed z-[100]',
        resolvedPlacement === 'absolute' && 'absolute z-20',
        resolvedPlacement !== 'static' && POSITION_CLASS[position],
        classNames?.root,
        className
      )}
    >
      <AnimatePresence initial={false} mode="popLayout">
        {visibleToasts.map((toast, index) => (
          <AnimatedToastItem
            key={toast.id}
            toast={toast}
            index={index}
            onDismiss={onDismiss}
            classNames={classNames}
            icons={icons}
            renderToast={renderToast}
            dismissLabel={dismissLabel}
          />
        ))}
      </AnimatePresence>
    </ol>
  )

  if (shouldPortal && !portalTarget) {
    return null
  }

  if (shouldPortal && portalTarget) {
    return createPortal(stack, portalTarget)
  }

  return stack
}
