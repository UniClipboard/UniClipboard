import React, { useId, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { HistoryTagColorDto, HistoryTagDto } from '@/api/daemon/history-tags'
import { useTagTints } from '@/components/history/tags/tag-colors-context'
import { DEFAULT_TAG_COLOR, isPresetColor, TAG_COLORS } from '@/lib/tag-colors'
import { cn } from '@/lib/utils'
import { MAX_TAG_NAME_LENGTH, tagSuggestions, type TagSuggestion } from './history-tag-suggestions'
import HistoryTagCreateOption from './HistoryTagCreateOption'

interface HistoryTagEditorProps {
  tags: HistoryTagDto[]
  attachedIds: ReadonlySet<string>
  /** Attach an existing tag or create-and-attach (in `color`); resolves to success. */
  onPick: (suggestion: TagSuggestion, color: HistoryTagColorDto) => Promise<boolean>
  onClose: () => void
}

/**
 * The detail column's inline tag field (HDetail.dc.html `tagging`): type to
 * filter this device's tags or name a new one; ↑↓ choose, ↵ attach, ⇥ cycle
 * a new tag's color, esc close.
 */
function HistoryTagEditor({ tags, attachedIds, onPick, onClose }: HistoryTagEditorProps) {
  const { t } = useTranslation()
  const tintOf = useTagTints()
  const listId = useId()
  const [query, setQuery] = useState('')
  const [highlight, setHighlight] = useState(0)
  const [busy, setBusy] = useState(false)
  const [color, setColor] = useState<HistoryTagColorDto>(DEFAULT_TAG_COLOR)
  // While the custom color popover is open, focus sits in it: the field's
  // blur must not close the editor.
  const [pickerOpen, setPickerOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const changePickerOpen = (open: boolean) => {
    setPickerOpen(open)
    if (open) return
    // Back to the name field, unless focus already moved elsewhere on purpose.
    requestAnimationFrame(() => {
      // Focus may still sit in the closing picker while it fades out.
      const focused = document.activeElement
      const nowhere =
        !focused ||
        focused === document.body ||
        focused.closest('[data-slot=popover-content]') !== null
      if (nowhere || rootRef.current?.contains(focused)) inputRef.current?.focus()
      else onClose()
    })
  }
  const suggestions = tagSuggestions(query, tags, attachedIds)
  const active = Math.min(highlight, suggestions.length - 1)
  const firstExisting = suggestions.findIndex(s => s.kind === 'existing')

  const pick = async (suggestion: TagSuggestion | undefined) => {
    if (!suggestion || busy) return
    setBusy(true)
    const ok = await onPick(suggestion, color)
    setBusy(false)
    if (ok) onClose()
  }

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      if (suggestions.length === 0) return
      const step = event.key === 'ArrowDown' ? 1 : -1
      setHighlight((active + step + suggestions.length) % suggestions.length)
    } else if (event.key === 'Enter') {
      event.preventDefault()
      void pick(suggestions[active])
    } else if (event.key === 'Tab' && suggestions[active]?.kind === 'create') {
      // ⇥ cycles the new tag's color (⇧⇥ backwards) instead of leaving the field.
      event.preventDefault()
      // A custom color sits past the palette's end, so ⇥ starts from its first.
      const step = event.shiftKey ? -1 : 1
      const at = isPresetColor(color)
        ? TAG_COLORS.indexOf(color)
        : step > 0
          ? -1
          : TAG_COLORS.length
      setColor(TAG_COLORS[(at + step + TAG_COLORS.length) % TAG_COLORS.length])
    } else if (event.key === 'Escape') {
      // Close the field only; the page's own Escape handling stays untouched.
      event.preventDefault()
      event.stopPropagation()
      onClose()
    }
  }

  return (
    <div ref={rootRef} className="relative">
      <label className="inline-flex h-6.5 w-37.5 items-center gap-1 rounded-full border-[1.5px] border-foreground bg-background px-2.5 text-ui-caption ring-3 ring-history-accent-soft">
        <span className="text-muted-foreground">#</span>
        <input
          type="text"
          role="combobox"
          aria-label={t('history.tags.inputLabel')}
          aria-expanded={suggestions.length > 0}
          aria-controls={listId}
          aria-activedescendant={suggestions.length > 0 ? `${listId}-${active}` : undefined}
          autoFocus
          maxLength={MAX_TAG_NAME_LENGTH}
          value={query}
          onChange={event => {
            setQuery(event.target.value)
            setHighlight(0)
          }}
          onKeyDown={handleKeyDown}
          ref={inputRef}
          onBlur={() => {
            if (!pickerOpen) onClose()
          }}
          className="w-full min-w-0 bg-transparent font-semibold text-foreground outline-none"
        />
      </label>
      <div
        className="absolute left-0 top-full z-50 mt-1.5 w-75 rounded-xl border border-border bg-popover p-1.5 text-popover-foreground shadow-[0_20px_48px_rgb(14_15_18/0.2)]"
        // Keep focus in the field while choosing with the pointer.
        onMouseDown={event => event.preventDefault()}
      >
        <div id={listId} role="listbox" aria-label={t('history.tags.suggestions')}>
          {suggestions.map((suggestion, index) => (
            <React.Fragment key={suggestion.kind === 'create' ? 'create' : suggestion.tag.tagId}>
              {index === firstExisting && (
                <div className="px-2.5 pb-1 pt-2 text-ui-caption font-semibold uppercase text-muted-foreground">
                  {t(
                    suggestion.kind === 'existing' && suggestion.group === 'similar'
                      ? 'history.tags.similar'
                      : 'history.tags.frequent'
                  )}
                </div>
              )}
              {suggestion.kind === 'create' ? (
                <HistoryTagCreateOption
                  id={`${listId}-${index}`}
                  name={suggestion.name}
                  color={color}
                  onColorChange={setColor}
                  active={index === active}
                  onPick={() => void pick(suggestion)}
                  onHighlight={() => setHighlight(index)}
                  pickerOpen={pickerOpen}
                  onPickerOpenChange={changePickerOpen}
                />
              ) : (
                <button
                  type="button"
                  id={`${listId}-${index}`}
                  role="option"
                  aria-selected={index === active}
                  tabIndex={-1}
                  onClick={() => void pick(suggestion)}
                  onMouseEnter={() => setHighlight(index)}
                  className={cn(
                    'flex h-8 w-full items-center gap-2 rounded-lg px-2.5 text-left text-ui-body',
                    index === active && 'bg-history-accent-soft'
                  )}
                >
                  <span
                    style={tintOf(suggestion.tag.tagId).style}
                    className={cn('size-2 shrink-0 rounded-full', tintOf(suggestion.tag.tagId).dot)}
                  />
                  <span className="min-w-0 truncate">#{suggestion.tag.name}</span>
                  <span className="ml-auto text-ui-caption text-muted-foreground">
                    {suggestion.tag.entryCount}
                  </span>
                  {index === active && (
                    <span className="font-mono text-ui-caption text-muted-foreground">↵</span>
                  )}
                </button>
              )}
            </React.Fragment>
          ))}
        </div>
        <p className="mt-0.5 border-t border-border/60 px-2.5 pb-1 pt-2 text-ui-caption text-muted-foreground">
          {t('history.tags.hint')}
        </p>
      </div>
    </div>
  )
}

export default HistoryTagEditor
