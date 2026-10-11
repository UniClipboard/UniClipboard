import { Input, makeStyles, tokens } from '@fluentui/react-components'
import { SearchRegular } from '@fluentui/react-icons'
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
}: SearchFieldProps) {
  const styles = useStyles()
  return (
    <Input
      type="search"
      className={styles.root}
      value={value}
      defaultValue={defaultValue}
      onChange={(_, data) => onValueChange?.(data.value)}
      placeholder={placeholder}
      aria-label={label}
      contentAfter={
        <span className={styles.after}>
          {shortcutHint ? (
            <kbd className={styles.hint} aria-hidden="true">
              {shortcutHint}
            </kbd>
          ) : null}
          <SearchRegular aria-hidden="true" />
        </span>
      }
    />
  )
}
