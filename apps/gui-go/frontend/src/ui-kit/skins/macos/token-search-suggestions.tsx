import type { TokenSearchFieldProps } from '@/ui-kit/contract'
import { Icon } from '@/ui-kit/skins/macos/icons'

type Props = Pick<
  TokenSearchFieldProps,
  | 'label'
  | 'suggestions'
  | 'highlighted'
  | 'onHighlightedChange'
  | 'onSelectSuggestion'
  | 'feedback'
  | 'suggestionsFooter'
> & { listId: string; open: boolean; optionId: (index: number) => string }

/** Suggestion rendering only; the shared search model owns completion and keyboard behavior. */
export function TokenSearchSuggestions({
  label,
  suggestions,
  highlighted,
  onHighlightedChange,
  onSelectSuggestion,
  feedback,
  suggestionsFooter,
  listId,
  open,
  optionId,
}: Props) {
  return (
    <div className="mac-menu mac-suggestions" hidden={!open}>
      {feedback && (
        <div id={`${listId}-feedback`} role="status" className="mac-suggestions-footer">
          {feedback}
        </div>
      )}
      <div id={listId} role="listbox" aria-label={label}>
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
      </div>
      {suggestionsFooter && <div className="mac-suggestions-footer">{suggestionsFooter}</div>}
    </div>
  )
}
