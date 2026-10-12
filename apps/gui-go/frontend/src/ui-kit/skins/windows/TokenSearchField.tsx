import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { AddRegular, DismissRegular, SearchRegular } from '@fluentui/react-icons'
import { useId } from 'react'
import type { SearchFieldToken, TokenSearchFieldProps } from '@/ui-kit/contract'

// Token colour families of boards WB1/WB2 (query chips of C3): source green, tag orange, time blue.
const KIND_TINT: Record<NonNullable<SearchFieldToken['kind']>, string> = {
  tag: 'orange',
  source: 'green',
  time: 'blue',
  type: 'purple',
  other: 'gray',
}
const tint = (kind: SearchFieldToken['kind']) => KIND_TINT[kind ?? 'other']

/**
 * WinUI AutoSuggestBox with query tokens (boards WB1, WB2, C3): the title-bar text box with the
 * conditions as removable chips before the caret, a clear button and the query glyph at the end,
 * the 2 px accent underline while focused, and a flyout of grouped suggestions with the selection
 * pill and a keyboard-hint footer. The ARIA combobox pattern: the input owns the listbox through
 * aria-controls/aria-activedescendant; the caller's search model decides what every key does.
 */
const useStyles = makeStyles({
  root: { position: 'relative', display: 'flex', minWidth: 0, width: '100%' },
  field: {
    boxSizing: 'border-box',
    width: '100%',
    minWidth: 0,
    height: '2rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.375rem',
    paddingInline: '0.375rem 0.5rem',
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: tokens.colorNeutralBackground1,
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderBottomColor: tokens.colorNeutralStrokeAccessible,
    // The accent underline of a focused text box, drawn inside so the field does not move.
    ':focus-within': {
      borderBottomColor: tokens.colorCompoundBrandStroke,
      boxShadow: `inset 0 -1px 0 ${tokens.colorCompoundBrandStroke}`,
    },
  },
  chip: {
    flexShrink: 0,
    height: '1.5rem',
    display: 'inline-flex',
    alignItems: 'center',
    gap: '0.25rem',
    paddingInline: '0.5rem 0.25rem',
    borderRadius: tokens.borderRadiusMedium,
    fontSize: tokens.fontSizeBase300,
    whiteSpace: 'nowrap',
  },
  chipField: { opacity: 0.8 },
  chipValue: { fontWeight: tokens.fontWeightSemibold },
  chipRemove: {
    width: '1.25rem',
    height: '1.25rem',
    padding: 0,
    border: 'none',
    borderRadius: tokens.borderRadiusSmall,
    background: 'transparent',
    color: 'inherit',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'default',
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
  },
  input: {
    flexGrow: 1,
    minWidth: '4rem',
    height: '100%',
    border: 'none',
    outline: 'none',
    background: 'transparent',
    color: tokens.colorNeutralForeground1,
    fontFamily: tokens.fontFamilyBase,
    fontSize: tokens.fontSizeBase300,
    '::placeholder': { color: tokens.colorNeutralForeground3 },
    '::-webkit-search-cancel-button': { display: 'none' },
  },
  hint: {
    flexShrink: 0,
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase100,
    color: tokens.colorNeutralForeground3,
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderRadius: tokens.borderRadiusSmall,
    paddingInline: tokens.spacingHorizontalXS,
  },
  iconButton: {
    flexShrink: 0,
    width: '1.5rem',
    height: '1.5rem',
    padding: 0,
    border: 'none',
    borderRadius: tokens.borderRadiusSmall,
    background: 'transparent',
    color: tokens.colorNeutralForeground2,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
  },
  glyph: { flexShrink: 0, color: tokens.colorNeutralForeground2, display: 'inline-flex' },
  // The suggestion flyout: acrylic layer, overlay radius, flyout elevation (boards T3, C5).
  flyout: {
    position: 'absolute',
    top: 'calc(100% + 0.25rem)',
    left: 0,
    right: 0,
    zIndex: 20,
    boxSizing: 'border-box',
    paddingBlock: '0.25rem',
    backgroundColor: tokens.colorNeutralBackground1,
    border: `${tokens.strokeWidthThin} solid var(--w11-stroke-elevation)`,
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow16,
    maxHeight: '24rem',
    overflowY: 'auto',
  },
  group: {
    paddingBlock: '0.5rem 0.25rem',
    paddingInline: '1rem',
    fontSize: tokens.fontSizeBase200,
    fontWeight: tokens.fontWeightSemibold,
    color: tokens.colorNeutralForeground2,
  },
  option: {
    position: 'relative',
    minHeight: '2.5rem',
    marginInline: '0.25rem',
    paddingInline: '0.75rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
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
  selected: { fontWeight: tokens.fontWeightSemibold },
  optionChip: {
    height: '1.5rem',
    display: 'inline-flex',
    alignItems: 'center',
    paddingInline: '0.5rem',
    borderRadius: tokens.borderRadiusMedium,
    fontWeight: tokens.fontWeightSemibold,
  },
  detail: {
    marginInlineStart: 'auto',
    color: tokens.colorNeutralForeground2,
    fontSize: tokens.fontSizeBase200,
    whiteSpace: 'nowrap',
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

const tintStyle = (kind: SearchFieldToken['kind']) => ({
  backgroundColor: `var(--w11-tag-${tint(kind)}-bg)`,
  color: `var(--w11-tag-${tint(kind)}-ink)`,
})

// The field is not inside a Toolbar today; should it ever be, the toolbar's arrow-key group must not take the
// caret keys away from text editing.
const editingKeys = {
  'data-tabster': JSON.stringify({
    focusable: { ignoreKeydown: { Home: true, End: true, ArrowLeft: true, ArrowRight: true } },
  }),
}

export function TokenSearchField({
  label,
  placeholder,
  shortcutHint,
  value,
  onValueChange,
  onSelectionChange,
  feedback,
  tokens: conditions,
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
  const styles = useStyles()
  const listId = useId()
  const optionId = (index: number) => `${listId}-${index}`
  const open = suggestionsOpen && (suggestions.length > 0 || Boolean(feedback))
  const empty = !value && conditions.length === 0
  return (
    <div className={styles.root} data-search-field="">
      <div className={styles.field}>
        {conditions.map(token => (
          <span key={token.id} className={styles.chip} style={tintStyle(token.kind)}>
            <span className={styles.chipField}>{`${token.field}:`}</span>
            <span className={styles.chipValue}>{token.value}</span>
            <button
              type="button"
              className={styles.chipRemove}
              aria-label={token.remove.label}
              // Keep the caret in the field while a chip is removed.
              onMouseDown={event => event.preventDefault()}
              onClick={() => token.remove.onClick()}
            >
              <DismissRegular fontSize={12} />
            </button>
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
          aria-describedby={open && feedback ? `${listId}-feedback` : undefined}
          aria-activedescendant={open && highlighted >= 0 ? optionId(highlighted) : undefined}
          placeholder={conditions.length > 0 ? undefined : placeholder}
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
          onKeyDown={event => {
            if (event.nativeEvent.isComposing || event.keyCode === 229) return
            onKeyDown?.({
              key: event.key,
              shiftKey: event.shiftKey,
              preventDefault: () => event.preventDefault(),
            })
            // A Tab the model leaves alone moves focus on. Close the flyout now, inside the key event, so the
            // page has settled before the browser moves focus; closing on blur instead re-renders while focus
            // passes the next toolbar's focus guard and drops it on <body>.
            if (event.key === 'Tab' && !event.defaultPrevented && open)
              onSuggestionsOpenChange(false)
          }}
          onFocus={() => onSuggestionsOpenChange(true)}
          onBlur={() => onSuggestionsOpenChange(false)}
          {...editingKeys}
        />
        {empty && shortcutHint ? (
          <kbd className={styles.hint} aria-hidden="true">
            {shortcutHint}
          </kbd>
        ) : null}
        {clear && !empty ? (
          <button
            type="button"
            className={styles.iconButton}
            aria-label={clear.label}
            onMouseDown={event => event.preventDefault()}
            onClick={() => clear.onClick()}
          >
            <DismissRegular fontSize={14} />
          </button>
        ) : null}
        <span className={styles.glyph} aria-hidden="true">
          <SearchRegular fontSize={16} />
        </span>
      </div>
      <div className={styles.flyout} hidden={!open}>
        {feedback && (
          <div id={`${listId}-feedback`} role="status" className={styles.footer}>
            {feedback}
          </div>
        )}
        <div id={listId} role="listbox" aria-label={label}>
          {suggestions.map((suggestion, index) => (
            <div key={suggestion.id} role="presentation">
              {suggestion.group ? (
                <div className={styles.group} role="presentation">
                  {suggestion.group}
                </div>
              ) : null}
              <div
                id={optionId(index)}
                role="option"
                aria-selected={index === highlighted}
                aria-disabled={suggestion.muted || undefined}
                className={mergeClasses(
                  styles.option,
                  index === highlighted && styles.highlighted,
                  suggestion.muted && styles.muted,
                  suggestion.selected && styles.selected
                )}
                // The field keeps focus; the pointer only chooses.
                onMouseDown={event => event.preventDefault()}
                onMouseEnter={() => onHighlightedChange(index)}
                onClick={() => onSelectSuggestion(index)}
              >
                {suggestion.create ? <AddRegular aria-hidden="true" fontSize={16} /> : null}
                {suggestion.kind && !suggestion.create ? (
                  <span className={styles.optionChip} style={tintStyle(suggestion.kind)}>
                    {suggestion.label}
                  </span>
                ) : (
                  <span>{suggestion.label}</span>
                )}
                {suggestion.detail ? (
                  <span className={styles.detail}>{suggestion.detail}</span>
                ) : null}
              </div>
            </div>
          ))}
        </div>
        {suggestionsFooter ? (
          <div className={styles.footer} aria-hidden="true">
            {suggestionsFooter}
          </div>
        ) : null}
      </div>
    </div>
  )
}
