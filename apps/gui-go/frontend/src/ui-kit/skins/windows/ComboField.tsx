import { makeStyles, tokens } from '@fluentui/react-components'
import { useId } from 'react'
import type { ComboFieldProps } from '@/ui-kit/contract'

// Minimal Windows rendering of the contract; the Win11 styling belongs to the Windows History work.
const useStyles = makeStyles({
  root: { position: 'relative', display: 'inline-flex' },
  input: {
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderRadius: tokens.borderRadiusMedium,
    paddingInline: tokens.spacingHorizontalS,
    font: 'inherit',
  },
  list: {
    position: 'absolute',
    top: '100%',
    left: 0,
    zIndex: 10,
    minWidth: '16rem',
    padding: tokens.spacingVerticalXS,
    backgroundColor: tokens.colorNeutralBackground1,
    boxShadow: tokens.shadow16,
    borderRadius: tokens.borderRadiusMedium,
  },
  option: { padding: tokens.spacingVerticalXS },
  highlighted: { backgroundColor: tokens.colorNeutralBackground1Selected },
})

export function ComboField({
  label,
  prefix,
  placeholder,
  value,
  onValueChange,
  suggestions,
  highlighted,
  onHighlightedChange,
  onSelectSuggestion,
  onKeyDown,
  onDismiss,
  maxLength,
  inputRef,
}: ComboFieldProps) {
  const styles = useStyles()
  const listId = useId()
  return (
    <div className={styles.root}>
      {prefix ? <span aria-hidden="true">{prefix}</span> : null}
      <input
        ref={inputRef}
        className={styles.input}
        type="text"
        role="combobox"
        aria-label={label}
        aria-expanded={suggestions.length > 0}
        aria-controls={listId}
        aria-activedescendant={highlighted >= 0 ? `${listId}-${highlighted}` : undefined}
        // eslint-disable-next-line jsx-a11y/no-autofocus
        autoFocus
        placeholder={placeholder}
        maxLength={maxLength}
        value={value}
        onChange={event => onValueChange(event.target.value)}
        onKeyDown={event => onKeyDown?.(event)}
        onBlur={onDismiss}
      />
      <div
        id={listId}
        role="listbox"
        aria-label={label}
        className={styles.list}
        hidden={suggestions.length === 0}
        onMouseDown={event => event.preventDefault()}
      >
        {suggestions.map((suggestion, index) => (
          <div
            key={suggestion.id}
            id={`${listId}-${index}`}
            role="option"
            aria-selected={index === highlighted}
            className={`${styles.option} ${index === highlighted ? styles.highlighted : ''}`}
            onMouseEnter={() => onHighlightedChange(index)}
            onClick={() => onSelectSuggestion(index)}
          >
            {suggestion.label}
          </div>
        ))}
      </div>
    </div>
  )
}
