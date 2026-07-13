// Adapted from beui.dev/components/motion/theme-toggle.
//
// The original drives next-themes and wraps the switch in its own View
// Transition. This project keeps theme in daemon-persisted settings
// (SettingContext / theme-engine) and already runs a centralized circle-blur
// reveal in `@/lib/theme-transition` whenever `setting.general.theme` changes.
// So here we only trigger the setting change (recording the click origin for
// the reveal) and keep beUI's ActionSwapIcon icon-swap visual.

import { Moon, Sun } from 'lucide-react'
import { useEffect, useState, type ComponentPropsWithoutRef } from 'react'
import { ActionSwapIcon } from '@/components/motion/action-swap'
import { useSetting } from '@/hooks/useSetting'
import { createLogger } from '@/lib/logger'
import { setTransitionOrigin } from '@/lib/theme-transition'
import { cn } from '@/lib/utils'

const log = createLogger('theme-toggle')

export interface ThemeToggleProps extends Omit<
  ComponentPropsWithoutRef<'button'>,
  'children' | 'onClick'
> {
  iconClassName?: string
}

/**
 * Read the resolved light/dark mode straight from the `dark` class that
 * SettingContext maintains on <html>. Staying on the root class keeps this in
 * sync with every path that changes the theme (settings page, system change),
 * without duplicating theme-engine's resolve logic.
 */
function useResolvedDark(): boolean {
  const [isDark, setIsDark] = useState(() => document.documentElement.classList.contains('dark'))

  useEffect(() => {
    const root = document.documentElement
    const sync = () => setIsDark(root.classList.contains('dark'))
    sync()
    const observer = new MutationObserver(sync)
    observer.observe(root, { attributes: true, attributeFilter: ['class'] })
    return () => observer.disconnect()
  }, [])

  return isDark
}

export function ThemeToggle({ className, iconClassName, ...rest }: ThemeToggleProps) {
  const { updateGeneralSetting } = useSetting()
  const isDark = useResolvedDark()

  const toggle = (e: React.MouseEvent<HTMLButtonElement>) => {
    // Record the click point so theme-transition's circle-blur reveal expands
    // from the button; SettingContext runs the transition on the setting change.
    setTransitionOrigin(e.clientX, e.clientY)
    updateGeneralSetting({ theme: isDark ? 'light' : 'dark' }).catch(err => {
      log.error({ err }, 'Failed to toggle theme')
    })
  }

  return (
    <button
      type="button"
      aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
      onClick={toggle}
      className={cn('flex items-center justify-center', className)}
      {...rest}
    >
      <ActionSwapIcon value={isDark ? 'dark' : 'light'} animation="blur" className={iconClassName}>
        {isDark ? <Sun className={iconClassName} /> : <Moon className={iconClassName} />}
      </ActionSwapIcon>
    </button>
  )
}
