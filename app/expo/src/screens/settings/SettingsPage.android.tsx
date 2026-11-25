import { Host, LazyColumn } from '@expo/ui/jetpack-compose';
import { fillMaxSize } from '@expo/ui/jetpack-compose/modifiers';
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';
import { useTheme } from '@/hooks/useTheme';
import { SettingsSectionItem } from '@/screens/settings/SettingsSectionItem';
import { SettingsListRow } from '@/screens/settings/android/SettingsListRow';
import { SettingsSwitchRow } from '@/screens/settings/android/SettingsSwitchRow';
import { SettingsSelectRow } from '@/screens/settings/android/SettingsSelectRow';
import { useFloatingNavigationClearance } from '@/components/android/floatingNavigationClearance';
import { MATERIAL_SEED_COLOR } from '@/theme/colors';
import type { SettingsPageProps } from './SettingsPage.types';

export function SettingsPage({ groups }: SettingsPageProps) {
  const { theme } = useTheme();
  const insets = useSafeAreaInsets();
  const clearance = useFloatingNavigationClearance();
  return (
    <SafeAreaView
      style={{ flex: 1, backgroundColor: theme.colors.background }}
      edges={[]}
    >
      <Host
        style={{ flex: 1 }}
        colorScheme={theme.isDark ? 'dark' : 'light'}
        seedColor={MATERIAL_SEED_COLOR}
      >
        <LazyColumn
          modifiers={[fillMaxSize()]}
          contentPadding={{
            start: 16,
            end: 16,
            top: 8,
            bottom: insets.bottom + clearance + 40,
          }}
          verticalArrangement={{ spacedBy: 16 }}
        >
          {groups.map((group, index) => (
            <SettingsSectionItem
              key={index}
              title={group.title}
              footer={group.footer}
              variant="grouped"
            >
              {group.rows.map((row) =>
                row.toggle ? (
                  <SettingsSwitchRow
                    key={row.id}
                    testID={row.id}
                    title={row.title}
                    description={row.description}
                    value={row.toggle.value}
                    onValueChange={row.toggle.onChange}
                  />
                ) : row.choices ? (
                  <SettingsSelectRow
                    key={row.id}
                    testID={row.id}
                    title={row.title}
                    description={row.description}
                    selectedValue={row.choices.value}
                    options={row.choices.options}
                    onSelect={row.choices.onChange}
                  />
                ) : (
                  <SettingsListRow
                    key={row.id}
                    testID={row.id}
                    title={row.title}
                    description={row.description}
                    onPress={row.onPress}
                    trailing={
                      row.value
                        ? { text: row.value, chevron: Boolean(row.onPress) }
                        : row.onPress
                          ? 'chevron'
                          : 'none'
                    }
                  />
                )
              )}
            </SettingsSectionItem>
          ))}
        </LazyColumn>
      </Host>
    </SafeAreaView>
  );
}
