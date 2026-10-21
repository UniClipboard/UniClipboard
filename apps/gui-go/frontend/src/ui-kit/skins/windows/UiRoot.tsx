import { FluentProvider, makeStyles, tokens } from '@fluentui/react-components'
import { MotionBehaviourProvider } from '@fluentui/react-motion'
import type { UiRootProps } from '@/ui-kit/contract'
import { useGuiVisualEffects } from '@/ui-kit/visual-effects'
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
  const { lowEffects, reduceMotion } = useGuiVisualEffects()
  const theme = {
    ...themes[colorScheme],
    ...(lowEffects
      ? {
          shadow2: 'none',
          shadow4: 'none',
          shadow8: 'none',
          shadow16: 'none',
          shadow28: 'none',
          shadow64: 'none',
        }
      : {}),
    ...(reduceMotion
      ? {
          durationUltraFast: '0ms',
          durationFaster: '0ms',
          durationFast: '0ms',
          durationNormal: '0ms',
          durationGentle: '0ms',
          durationSlow: '0ms',
          durationSlower: '0ms',
          durationUltraSlow: '0ms',
        }
      : {}),
  }
  return (
    <MotionBehaviourProvider value={reduceMotion ? 'skip' : 'default'}>
      <FluentProvider
        theme={theme}
        className={styles.root}
        style={fill}
        data-ui-skin="windows"
        data-color-scheme={colorScheme}
      >
        {children}
      </FluentProvider>
    </MotionBehaviourProvider>
  )
}
