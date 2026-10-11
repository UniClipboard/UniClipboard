import { Input, makeStyles, tokens } from '@fluentui/react-components'
import { SearchRegular } from '@fluentui/react-icons'
import { useMemo } from 'react'
import type { SearchFieldProps } from '@/ui-kit/contract'

const useStyles = makeStyles({
  root: { width: '100%', maxWidth: '100%' },
  after: { display: 'flex', alignItems: 'center', gap: '0.5rem' },
  hint: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: tokens.fontSizeBase100,
    color: tokens.colorNeutralForeground3,
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
    borderRadius: tokens.borderRadiusSmall,
    paddingInline: tokens.spacingHorizontalXS,
  },
})

/**
 * WinUI AutoSuggestBox in the title bar: query glyph at the end, 2px accent underline on focus.
 * Fluent SearchBox puts the glyph first and hides trailing content at rest, so this uses Input.
 */
export function SearchField({
  value,
  defaultValue,
  onValueChange,
  placeholder,
  label,
  shortcutHint,
  onSubmit,
  inputRef,
}: SearchFieldProps) {
  const styles = useStyles()
  // Memoized so the Input does not receive a new contentAfter element on every keystroke.
  const after = useMemo(
    () => (
      <span className={styles.after}>
        {shortcutHint ? (
          <kbd className={styles.hint} aria-hidden="true">
            {shortcutHint}
          </kbd>
        ) : null}
        <SearchRegular aria-hidden="true" />
      </span>
    ),
    [shortcutHint, styles.after, styles.hint]
  )
  return (
    <Input
      // Fluent Input forwards `ref` to the native <input>.
      ref={inputRef}
      type="search"
      className={styles.root}
      root={{ 'data-search-field': '' } as Record<string, string>}
      value={value}
      defaultValue={defaultValue}
      onChange={(_, data) => onValueChange?.(data.value)}
      onKeyDown={event => {
        // Enter while an IME composition is open commits the composition, not the search.
        if (event.key !== 'Enter' || event.nativeEvent.isComposing || !onSubmit) return
        event.preventDefault()
        onSubmit(event.currentTarget.value)
      }}
      placeholder={placeholder}
      aria-label={label}
      contentAfter={after}
    />
  )
}
