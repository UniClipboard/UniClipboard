import { useId, type KeyboardEvent } from 'react'
import type { TokenSearchFieldProps } from '@/ui-kit/contract'
import { Icon } from '@/ui-kit/skins/linux/icons'
import { isImeKey } from '@/ui-kit/skins/linux/ime'
import { TokenSearchSuggestions } from '@/ui-kit/skins/linux/token-search-suggestions'

/**
 * Header bar search entry with search tokens: conditions sit inside the entry as coloured tokens,
 * the entry widens while it is in use, and suggestions drop down in a popover whose first match is
 * highlighted. The suggestion keys belong to the caller's search model,
 * which sees every key first through `onKeyDown`.
 */
export function TokenSearchField({
  label,
  placeholder,
  shortcutHint,
  value,
  onValueChange,
  onSelectionChange,
  feedback,
  tokens,
  suggestions,
  suggestionsOpen,
  onSuggestionsOpenChange,
  highlighted,
  onHighlightedChange,
  onSelectSuggestion,
  onKeyDown,
  clear,
  suggestionsFooter,
  inputRef,
}: TokenSearchFieldProps) {
  const listId = useId()
  const optionId = (index: number) => `${listId}-option-${index}`
  const open = suggestionsOpen && (suggestions.length > 0 || Boolean(feedback))
  const active = value.length > 0 || tokens.length > 0
  const keyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (isImeKey(event)) return
    onKeyDown?.(event)
  }
  return (
    <div className="adw-token-search" data-active={active ? '' : undefined}>
      <div className="adw-search">
        <Icon name="search" size={16} />
        {tokens.map(token => (
          <span key={token.id} className="adw-token" data-kind={token.kind}>
            <span className="adw-token-field">{token.field}</span>
            <span className="adw-token-value">{token.value}</span>
            <button
              type="button"
              className="adw-tag-remove"
              aria-label={token.remove.label}
              onClick={token.remove.onClick}
            >
              <Icon name="close" size={10} strokeWidth={2.4} />
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          type="search"
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-describedby={open && feedback ? `${listId}-feedback` : undefined}
          aria-activedescendant={open && highlighted >= 0 ? optionId(highlighted) : undefined}
          placeholder={tokens.length > 0 ? undefined : placeholder}
          value={value}
          onChange={event =>
            onValueChange(event.target.value, {
              start: event.target.selectionStart ?? event.target.value.length,
              end: event.target.selectionEnd ?? event.target.value.length,
            })
          }
          onSelect={event =>
            onSelectionChange?.(
              event.currentTarget.selectionStart ?? 0,
              event.currentTarget.selectionEnd ?? 0
            )
          }
          onKeyDown={keyDown}
          onFocus={() => onSuggestionsOpenChange(true)}
          onBlur={() => onSuggestionsOpenChange(false)}
        />
        {!active && shortcutHint && <kbd>{shortcutHint}</kbd>}
        {active && clear && (
          <button
            type="button"
            className="adw-search-clear"
            aria-label={clear.label}
            // Keep focus in the field.
            onMouseDown={event => event.preventDefault()}
            onClick={clear.onClick}
          >
            <Icon name="close" size={10} strokeWidth={2.6} />
          </button>
        )}
      </div>
      <TokenSearchSuggestions
        label={label}
        suggestions={suggestions}
        highlighted={highlighted}
        onHighlightedChange={onHighlightedChange}
        onSelectSuggestion={onSelectSuggestion}
        feedback={feedback}
        suggestionsFooter={suggestionsFooter}
        listId={listId}
        open={open}
        optionId={optionId}
      />
    </div>
  )
}
