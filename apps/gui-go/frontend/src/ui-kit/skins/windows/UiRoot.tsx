import { FluentProvider, makeStyles, tokens } from '@fluentui/react-components'
import type { UiRootProps } from '@/ui-kit/contract'
import { themes } from './theme'

const useStyles = makeStyles({
  root: {
    backgroundColor: 'var(--w11-base)',
    color: tokens.colorNeutralForeground1,
    fontFamily: tokens.fontFamilyBase,
  },
})

/** FluentProvider with the Win11 theme. Portals (menus, dialogs, tooltips) inherit it. */
export function UiRoot({ colorScheme, children }: UiRootProps) {
  const styles = useStyles()
  return (
    <FluentProvider
      theme={themes[colorScheme]}
      className={styles.root}
      data-ui-skin="windows"
      data-color-scheme={colorScheme}
    >
      {children}
    </FluentProvider>
  )
}
