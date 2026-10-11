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

// The root fills the page region it is mounted in, so a full-window SplitView can take its height. It is
// inline: FluentProvider copies its className onto every portal mount node under <body>, where a full
// height would cover the page.
const fill = { height: '100%' }

/** FluentProvider with the Win11 theme. Portals (menus, dialogs, tooltips) inherit it. */
export function UiRoot({ colorScheme, children }: UiRootProps) {
  const styles = useStyles()
  return (
    <FluentProvider
      theme={themes[colorScheme]}
      className={styles.root}
      style={fill}
      data-ui-skin="windows"
      data-color-scheme={colorScheme}
    >
      {children}
    </FluentProvider>
  )
}
