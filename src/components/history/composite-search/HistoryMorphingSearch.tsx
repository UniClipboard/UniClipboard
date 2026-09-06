import { AnimatePresence, LayoutGroup, m } from 'framer-motion'
import { Search, X } from 'lucide-react'
import {
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useId,
  useRef,
} from 'react'
import { SPRING_LAYOUT } from '@/lib/ease'

interface HistoryMorphingSearchProps {
  open: boolean
  active: boolean
  containerRef: React.RefObject<HTMLDivElement | null>
  inputRef: React.RefObject<HTMLInputElement | null>
  value: string
  suggestionsOpen: boolean
  suggestionsId: string
  title: string
  placeholder: string
  resultsLabel: string
  clearAllLabel: string
  onInputChange: (event: ChangeEvent<HTMLInputElement>) => void
  onInputKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void
  onClearAll: () => void
  onOpenChange: (open: boolean) => void
  children: ReactNode
}

function HistoryMorphingSearch({
  open,
  active,
  containerRef,
  inputRef,
  value,
  suggestionsOpen,
  suggestionsId,
  title,
  placeholder,
  resultsLabel,
  clearAllLabel,
  onInputChange,
  onInputKeyDown,
  onClearAll,
  onOpenChange,
  children,
}: HistoryMorphingSearchProps) {
  const uid = useId()
  const layoutId = `${uid}-history-search-surface`
  const triggerRef = useRef<HTMLButtonElement>(null)
  const previousFocusRef = useRef<HTMLElement | null>(null)
  const wasOpenRef = useRef(open)

  useEffect(() => {
    const wasOpen = wasOpenRef.current
    wasOpenRef.current = open

    if (open && !wasOpen) {
      const activeElement = document.activeElement
      previousFocusRef.current =
        activeElement instanceof HTMLElement && activeElement !== document.body
          ? activeElement
          : null
      return
    }

    if (!open && wasOpen) {
      const frame = requestAnimationFrame(() => {
        const previousFocus = previousFocusRef.current
        const focusTarget = previousFocus?.isConnected ? previousFocus : triggerRef.current
        focusTarget?.focus()
      })
      return () => cancelAnimationFrame(frame)
    }
  }, [open])

  return (
    <LayoutGroup id={uid}>
      <div
        ref={containerRef}
        data-testid="history-search-anchor"
        data-tauri-drag-region="false"
        className="relative z-50 size-8 shrink-0"
      >
        <AnimatePresence initial={false} mode="popLayout">
          {open ? (
            <m.section
              key="history-search-surface"
              layoutId={layoutId}
              data-testid="history-search-surface"
              role="dialog"
              aria-label={title}
              className="glass-strong absolute right-0 top-0 z-50 flex w-96 max-w-[calc(100vw-1.5rem)] flex-col overflow-hidden rounded-xl border border-border/80 text-foreground shadow-xl"
              transition={SPRING_LAYOUT}
            >
              <div className="flex h-12 shrink-0 items-center gap-2.5 border-b border-border/70 px-3.5">
                <Search className="size-4 shrink-0 text-muted-foreground" />
                <input
                  ref={inputRef}
                  type="text"
                  role="combobox"
                  aria-label={placeholder}
                  aria-expanded={suggestionsOpen}
                  aria-autocomplete="list"
                  aria-controls={suggestionsOpen ? suggestionsId : undefined}
                  autoCorrect="off"
                  autoCapitalize="off"
                  autoComplete="off"
                  spellCheck={false}
                  value={value}
                  onChange={onInputChange}
                  onKeyDown={onInputKeyDown}
                  placeholder={placeholder}
                  className="h-10 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
                />
                <span className="max-w-20 shrink-0 truncate text-xs text-muted-foreground">
                  {resultsLabel}
                </span>
                <button
                  type="button"
                  aria-label={clearAllLabel}
                  onClick={() => {
                    onClearAll()
                    onOpenChange(false)
                  }}
                  className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                >
                  <X className="size-4" />
                </button>
              </div>
              <m.div
                initial={{ opacity: 0, y: -4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -4 }}
                transition={{ duration: 0.16 }}
              >
                {children}
              </m.div>
            </m.section>
          ) : (
            <m.button
              ref={triggerRef}
              key="history-search-trigger"
              layoutId={layoutId}
              type="button"
              aria-label={title}
              aria-haspopup="dialog"
              aria-expanded="false"
              onClick={() => onOpenChange(true)}
              className="relative flex size-8 items-center justify-center rounded-full bg-muted/50 text-foreground outline-none transition-colors hover:bg-muted/70"
              transition={SPRING_LAYOUT}
            >
              <Search className="size-4 opacity-80" />
              {active && (
                <span
                  aria-hidden
                  className="absolute right-1 top-1 size-1.5 rounded-full bg-primary"
                />
              )}
            </m.button>
          )}
        </AnimatePresence>
      </div>
    </LayoutGroup>
  )
}

export default HistoryMorphingSearch
