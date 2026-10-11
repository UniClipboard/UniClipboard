import { makeStyles, tokens } from '@fluentui/react-components'
import { useId } from 'react'
import type { TokenSearchFieldProps } from '@/ui-kit/contract'
import { RemoveButton } from './RemoveButton'
import { useTagStyles } from './tag-styles'

// Minimal Windows rendering of the contract (combobox semantics, tokens, suggestion list); the
// Win11 visual design of this field belongs to the Windows History work.
const useStyles = makeStyles({
  root: { position: 'relative', display: 'flex', minWidth: 0 },
  field: {
    display: 'flex',
    alignItems: 'center',
    gap: tokens.spacingHorizontalXS,
    minWidth: 0,
    width: '100%',
    paddingInline: tokens.spacingHorizontalS,
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground1,
  },
  input: {
    flexGrow: 1,
    minWidth: 0,
    border: 'none',
    outline: 'none',
    background: 'transparent',
    color: tokens.colorNeutralForeground1,
    font: 'inherit',
    paddingBlock: tokens.spacingVerticalXS,
  },
  list: {
    position: 'absolute',
    top: '100%',
    left: 0,
    right: 0,
    zIndex: 10,
    marginTop: tokens.spacingVerticalXS,
    padding: tokens.spacingVerticalXS,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow16,
    borderRadius: tokens.borderRadiusMedium,
  },
  option: { display: 'flex', gap: tokens.spacingHorizontalS, padding: tokens.spacingVerticalXS },
  highlighted: { backgroundColor: tokens.colorNeutralBackground1Selected },
  group: { color: tokens.colorNeutralForeground3, fontSize: tokens.fontSizeBase200 },
  detail: { color: tokens.colorNeutralForeground3 },
})

export function TokenSearchField({
  label,
  placeholder,
  value,
  onValueChange,
  tokens: conditions,
  suggestions,
  suggestionsOpen,
  onSuggestionsOpenChange,
  highlighted,
  onHighlightedChange,
  onSelectSuggestion,
  onKeyDown,
  clear,
  inputRef,
}: TokenSearchFieldProps) {
  const styles = useStyles()
  const tagStyles = useTagStyles()
  const listId = useId()
  const optionId = (index: number) => `${listId}-${index}`
  const open = suggestionsOpen && suggestions.length > 0
  return (
    <div className={styles.root}>
      <div className={styles.field}>
        {conditions.map(token => (
          <span key={token.id} className={`${tagStyles.chip} ${tagStyles.token}`}>
            <span className={tagStyles.field}>{token.field}</span>
            <span className={tagStyles.value}>{token.value}</span>
            <RemoveButton action={token.remove} />
          </span>
        ))}
        <input
          ref={inputRef}
          className={styles.input}
          type="search"
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={open && highlighted >= 0 ? optionId(highlighted) : undefined}
          placeholder={conditions.length > 0 ? undefined : placeholder}
          value={value}
          onChange={event => onValueChange(event.target.value)}
          onKeyDown={event => onKeyDown?.(event)}
          onFocus={() => onSuggestionsOpenChange(true)}
          onBlur={() => onSuggestionsOpenChange(false)}
        />
        {clear && (value || conditions.length > 0) ? <RemoveButton action={clear} /> : null}
      </div>
      <div id={listId} role="listbox" aria-label={label} className={styles.list} hidden={!open}>
        {suggestions.map((suggestion, index) => (
          <div key={suggestion.id} role="presentation">
            {suggestion.group ? <div className={styles.group}>{suggestion.group}</div> : null}
            <div
              id={optionId(index)}
              role="option"
              aria-selected={index === highlighted}
              className={`${styles.option} ${index === highlighted ? styles.highlighted : ''}`}
              onMouseDown={event => event.preventDefault()}
              onMouseEnter={() => onHighlightedChange(index)}
              onClick={() => onSelectSuggestion(index)}
            >
              <span>{suggestion.label}</span>
              {suggestion.detail ? (
                <span className={styles.detail}>{suggestion.detail}</span>
              ) : null}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
