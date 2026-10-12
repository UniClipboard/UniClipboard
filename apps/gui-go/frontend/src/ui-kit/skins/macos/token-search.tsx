import { useId, type KeyboardEvent } from 'react'
import type { TokenSearchFieldProps } from '@/ui-kit/contract'
import { Icon } from './icons'
import { isImeKey } from './ime'

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
  const open = suggestionsOpen && suggestions.length > 0
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
          aria-activedescendant={open && highlighted >= 0 ? optionId(highlighted) : undefined}
          placeholder={tokens.length > 0 ? undefined : placeholder}
          value={value}
          onChange={event => onValueChange(event.target.value)}
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
      <div
        id={listId}
        role="listbox"
        aria-label={label}
        className="mac-menu mac-suggestions"
        hidden={!open}
      >
        {suggestions.map((suggestion, index) => (
          <div key={suggestion.id} role="presentation">
            {suggestion.group && <div className="mac-suggestion-group">{suggestion.group}</div>}
            <div
              id={optionId(index)}
              role="option"
              aria-selected={index === highlighted}
              className="mac-suggestion"
              data-muted={suggestion.muted ? '' : undefined}
              data-create={suggestion.create ? '' : undefined}
              // Keep focus in the field so the menu stays open.
              onMouseDown={event => event.preventDefault()}
              onMouseEnter={() => onHighlightedChange(index)}
              onClick={() => onSelectSuggestion(index)}
            >
              {suggestion.create ? (
                <span className="mac-suggestion-create">
                  <Icon name="plus" size={12} />
                  {suggestion.label}
                </span>
              ) : (
                <span className="mac-token" data-kind={suggestion.kind}>
                  {suggestion.label}
                </span>
              )}
              {suggestion.detail && (
                <span className="mac-suggestion-detail">{suggestion.detail}</span>
              )}
              {suggestion.selected && <Icon name="check" size={12} strokeWidth={2.4} />}
            </div>
          </div>
        ))}
        {suggestionsFooter && <div className="mac-suggestions-footer">{suggestionsFooter}</div>}
      </div>
    </div>
  )
}
