// Production theme application and skin rendering, without a daemon or native shell.
import { createRoot } from 'react-dom/client'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import {
  applyDesktopTheme,
  applyThemeOverrides,
  applyThemePreset,
  themePresets,
} from '@/lib/theme-engine'
import {
  Button,
  Checkbox,
  ItemList,
  Menu,
  SettingsContent,
  SettingsGroup,
  SettingsRow,
  Surface,
  Switch,
  UiRoot,
} from '@/ui-kit'
const scheme = new URLSearchParams(location.search).get('scheme') === 'dark' ? 'dark' : 'light'
const apply = (
  name: string,
  mode: 'light' | 'dark',
  desktop = false,
  overrides: Record<string, string> = {}
) => {
  const root = document.documentElement
  root.className = mode
  applyThemePreset(name, mode, root)
  applyThemeOverrides(overrides, root)
  if (desktop)
    applyDesktopTheme(
      Object.fromEntries(
        Array.from(root.style).map(key => [key, root.style.getPropertyValue(key)])
      ),
      root
    )
}
apply('default', scheme)
Object.assign(window, { themeFixture: { apply, presets: themePresets } })
export default function ThemePaletteFixture() {
  const mode = useAppliedColorScheme()
  return (
    <UiRoot colorScheme={mode}>
      <Surface region="content">
        <div
          style={{
            padding: '2rem',
            display: 'flex',
            flexDirection: 'column',
            gap: '1rem',
          }}
        >
          <Button data-testid="primary" variant="primary">
            Primary action
          </Button>
          <Switch label="Follow theme" checked onCheckedChange={() => {}} />
          <Checkbox label="Selected choice" checked onCheckedChange={() => {}} />
          <ItemList
            label="Entries"
            items={[{ id: 'one', title: 'Selected entry' }]}
            selection={['one']}
          />
          <Menu label="Open menu" items={[{ id: 'one', label: 'Menu item' }]} />
          <SettingsContent label="Theme settings">
            <SettingsGroup title="Theme">
              <SettingsRow label="Follow theme in settings">
                <Switch label="Settings switch" checked onCheckedChange={() => {}} />
              </SettingsRow>
              <SettingsRow label="Action">
                <Button variant="primary">Settings action</Button>
              </SettingsRow>
            </SettingsGroup>
          </SettingsContent>
        </div>
      </Surface>
    </UiRoot>
  )
}
createRoot(document.getElementById('root')!).render(<ThemePaletteFixture />)
