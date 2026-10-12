import { useId, type KeyboardEvent } from 'react'
import type { TokenSearchFieldProps } from '@/ui-kit/contract'
import { Icon } from '@/ui-kit/skins/macos/icons'
import { isImeKey } from '@/ui-kit/skins/macos/ime'
import { TokenSearchSuggestions } from '@/ui-kit/skins/macos/token-search-suggestions'

/**
 * Toolbar search field with search tokens (design MB1/MB2): conditions sit inside the glass capsule
 * as coloured tokens, the field widens while it is in use, and suggestions drop down in a menu
 * whose first match is accent-highlighted. The suggestion keys belong to the caller's search model,
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
    <div className="mac-token-search" data-active={active ? '' : undefined}>
      <div className="mac-search mac-glass">
        <Icon name="search" size={14} />
        {tokens.map(token => (
          <span key={token.id} className="mac-token" data-kind={token.kind}>
            <span className="mac-token-field">{token.field}</span>
            <span className="mac-token-value">{token.value}</span>
            <button
              type="button"
              className="mac-tag-remove"
              aria-label={token.remove.label}
              onClick={token.remove.onClick}
            >
              <Icon name="close" size={9} strokeWidth={2.4} />
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
            className="mac-search-clear"
            aria-label={clear.label}
            // Keep focus in the field.
            onMouseDown={event => event.preventDefault()}
            onClick={clear.onClick}
          >
            <Icon name="close" size={8} strokeWidth={2.8} />
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
