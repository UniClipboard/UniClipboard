import type { TokenSearchFieldProps } from '@/ui-kit/contract'
import { Icon } from '@/ui-kit/skins/linux/icons'

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
    <div className="adw-menu adw-suggestions" hidden={!open}>
      {feedback && (
        <div id={`${listId}-feedback`} role="status" className="adw-suggestions-footer">
          {feedback}
        </div>
      )}
      <div id={listId} role="listbox" aria-label={label}>
        {suggestions.map((suggestion, index) => (
          <div key={suggestion.id} role="presentation">
            {suggestion.group && <div className="adw-suggestion-group">{suggestion.group}</div>}
            <div
              id={optionId(index)}
              role="option"
              aria-selected={index === highlighted}
              className="adw-suggestion"
              data-muted={suggestion.muted ? '' : undefined}
              data-create={suggestion.create ? '' : undefined}
              // Keep focus in the field so the menu stays open.
              onMouseDown={event => event.preventDefault()}
              onMouseEnter={() => onHighlightedChange(index)}
              onClick={() => onSelectSuggestion(index)}
            >
              {suggestion.create ? (
                <span className="adw-suggestion-create">
                  <Icon name="plus" size={14} />
                  {suggestion.label}
                </span>
              ) : (
                <span className="adw-token" data-kind={suggestion.kind}>
                  {suggestion.label}
                </span>
              )}
              {suggestion.detail && (
                <span className="adw-suggestion-detail">{suggestion.detail}</span>
              )}
              {suggestion.selected && <Icon name="check" size={14} strokeWidth={2.2} />}
            </div>
          </div>
        ))}
      </div>
      {suggestionsFooter && <div className="adw-suggestions-footer">{suggestionsFooter}</div>}
    </div>
  )
}
