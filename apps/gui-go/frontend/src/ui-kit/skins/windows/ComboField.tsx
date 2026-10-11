import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { AddRegular } from '@fluentui/react-icons'
import { useId } from 'react'
import type { ComboFieldProps, TagColor } from '@/ui-kit/contract'
import { swatch } from './theme'

/**
 * The inline tag field of board WC1: a compact WinUI text box with its `#` prefix and the accent
 * underline, and a flyout below it with the "Create" row (and its colour choice), the matching
 * tags with their counts, and a keyboard-hint footer. The ARIA combobox pattern; the caller's model
 * decides what every key does, and leaving the field calls `onDismiss`.
 */
const useStyles = makeStyles({
  root: { position: 'relative', display: 'inline-flex', minWidth: '16rem' },
  field: {
    boxSizing: 'border-box',
    width: '100%',
    height: '1.75rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.125rem',
    paddingInline: '0.5rem',
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground1,
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderBottomColor: tokens.colorCompoundBrandStroke,
    boxShadow: `inset 0 -1px 0 ${tokens.colorCompoundBrandStroke}`,
  },
  prefix: { color: tokens.colorNeutralForeground3, fontSize: tokens.fontSizeBase300 },
  input: {
    flexGrow: 1,
    minWidth: 0,
    border: 'none',
    outline: 'none',
    background: 'transparent',
    color: tokens.colorNeutralForeground1,
    fontFamily: tokens.fontFamilyBase,
    fontSize: tokens.fontSizeBase300,
    fontWeight: tokens.fontWeightSemibold,
    '::placeholder': {
      color: tokens.colorNeutralForeground3,
      fontWeight: tokens.fontWeightRegular,
    },
  },
  flyout: {
    position: 'absolute',
    top: 'calc(100% + 0.25rem)',
    insetInlineStart: 0,
    zIndex: 20,
    boxSizing: 'border-box',
    width: '23rem',
    maxWidth: '80vw',
    paddingBlock: '0.25rem',
    backgroundColor: tokens.colorNeutralBackground1,
    border: `${tokens.strokeWidthThin} solid var(--w11-stroke-elevation)`,
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow16,
  },
  option: {
    position: 'relative',
    marginInline: '0.25rem',
    minHeight: '2.25rem',
    paddingInline: '0.75rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.625rem',
    borderRadius: tokens.borderRadiusMedium,
    fontSize: tokens.fontSizeBase300,
    cursor: 'default',
  },
  highlighted: {
    backgroundColor: tokens.colorSubtleBackgroundHover,
    '::before': {
      content: '""',
      position: 'absolute',
      insetInlineStart: 0,
      top: 'calc(50% - 0.5rem)',
      width: '0.1875rem',
      height: '1rem',
      borderRadius: '0.125rem',
      backgroundColor: tokens.colorBrandBackground,
    },
  },
  muted: { color: tokens.colorNeutralForeground3 },
  detail: {
    marginInlineStart: 'auto',
    color: tokens.colorNeutralForeground2,
    fontSize: tokens.fontSizeBase200,
    whiteSpace: 'nowrap',
  },
  colors: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.375rem',
    paddingBlock: '0 0.5rem',
    paddingInline: '1rem',
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
  },
  swatch: {
    width: '1.125rem',
    height: '1.125rem',
    padding: 0,
    borderRadius: tokens.borderRadiusCircular,
    border: `2px solid transparent`,
    outline: 'none',
    cursor: 'default',
    ':focus-visible': { outline: `${tokens.strokeWidthThick} solid ${tokens.colorStrokeFocus2}` },
  },
  swatchChosen: {
    boxShadow: `0 0 0 2px ${tokens.colorNeutralBackground1}, 0 0 0 3.5px currentColor`,
  },
  footer: {
    marginTop: '0.25rem',
    paddingBlock: '0.5rem 0.25rem',
    paddingInline: '1rem',
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
  },
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
  createColor,
  maxLength,
  footer,
  inputRef,
}: ComboFieldProps) {
  const styles = useStyles()
  const listId = useId()
  const open = suggestions.length > 0
  const colorRow = (choice: NonNullable<typeof createColor>) => (
    <div className={styles.colors} role="radiogroup" aria-label={choice.label}>
      <span>{choice.label}</span>
      {choice.colors.map((color: TagColor) => (
        <button
          key={color}
          type="button"
          role="radio"
          aria-checked={color === choice.value}
          aria-label={color}
          className={mergeClasses(styles.swatch, color === choice.value && styles.swatchChosen)}
          style={{ backgroundColor: swatch(color), color: swatch(color) }}
          // The field keeps focus while a colour is chosen.
          onMouseDown={event => event.preventDefault()}
          onClick={() => choice.onValueChange(color)}
        />
      ))}
    </div>
  )
  return (
    <div className={styles.root}>
      <div className={styles.field}>
        {prefix ? (
          <span className={styles.prefix} aria-hidden="true">
            {prefix}
          </span>
        ) : null}
        <input
          ref={inputRef}
          className={styles.input}
          type="text"
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={open && highlighted >= 0 ? `${listId}-${highlighted}` : undefined}
          // eslint-disable-next-line jsx-a11y/no-autofocus -- the field opens on a command (T, Add tag) to be typed in
          autoFocus
          placeholder={placeholder}
          maxLength={maxLength}
          value={value}
          onChange={event => onValueChange(event.target.value)}
          onKeyDown={event =>
            onKeyDown?.({
              key: event.key,
              shiftKey: event.shiftKey,
              preventDefault: () => event.preventDefault(),
            })
          }
          onBlur={onDismiss}
        />
      </div>
      <div
        id={listId}
        role="listbox"
        aria-label={label}
        className={styles.flyout}
        hidden={!open}
        onMouseDown={event => event.preventDefault()}
      >
        {suggestions.map((suggestion, index) => (
          <div key={suggestion.id} role="presentation">
            <div
              id={`${listId}-${index}`}
              role="option"
              aria-selected={index === highlighted}
              className={mergeClasses(
                styles.option,
                index === highlighted && styles.highlighted,
                suggestion.muted && styles.muted
              )}
              onMouseEnter={() => onHighlightedChange(index)}
              onClick={() => onSelectSuggestion(index)}
            >
              {/* WC1 draws each existing tag's colour dot; suggestions carry no colour in the contract yet. */}
              {suggestion.create ? <AddRegular aria-hidden="true" fontSize={16} /> : null}
              <span>{suggestion.label}</span>
              {suggestion.detail ? (
                <span className={styles.detail}>{suggestion.detail}</span>
              ) : null}
            </div>
            {suggestion.create && createColor ? colorRow(createColor) : null}
          </div>
        ))}
        {footer ? (
          <div className={styles.footer} aria-hidden="true">
            {footer}
          </div>
        ) : null}
      </div>
    </div>
  )
}
