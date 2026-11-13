import {
  Column,
  Host,
  Icon,
  IconButton,
  LazyColumn,
  Row,
  Text as ComposeText,
} from "@expo/ui/jetpack-compose";
import {
  fillMaxSize,
  padding,
  weight,
  rotate,
  testID as testIDModifier,
} from "@expo/ui/jetpack-compose/modifiers";
import { SafeAreaView } from "react-native-safe-area-context";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/hooks/useTheme";
import { SettingsSectionItem } from "@/screens/settings/SettingsSectionItem";
import { SettingsListRow } from "@/screens/settings/android/SettingsListRow";
import { SettingsSwitchRow } from "@/screens/settings/android/SettingsSwitchRow";
import { SettingsSelectRow } from "@/screens/settings/android/SettingsSelectRow";
import { useFloatingNavigationClearance } from "@/components/android/floatingNavigationClearance";
import type { SettingsPageProps } from "./SettingsPage.types";

export function SettingsPage({ title, groups, onBack }: SettingsPageProps) {
  const { theme } = useTheme();
  const { t } = useTranslation();
  const clearance = useFloatingNavigationClearance();
  return (
    <SafeAreaView
      style={{ flex: 1, backgroundColor: theme.colors.background }}
      edges={["top", "left", "right"]}
    >
      <Host style={{ flex: 1 }} colorScheme={theme.isDark ? "dark" : "light"}>
        <Column modifiers={[fillMaxSize()]}>
          <Row modifiers={[padding(16, 8, 16, 8)]} verticalAlignment="center">
            {onBack && (
              <IconButton
                modifiers={[testIDModifier("settings-back")]}
                onClick={onBack}
              >
                <Icon
                  source={require("../assets/icons/chevron_right.xml")}
                  contentDescription={t("common:action.back")}
                  modifiers={[rotate(180)]}
                />
              </IconButton>
            )}
            <ComposeText
              color={theme.colors.textPrimary as string}
              style={{ fontSize: 28, fontWeight: "700" }}
              modifiers={[weight(1)]}
            >
              {title}
            </ComposeText>
          </Row>
          <LazyColumn
            modifiers={[weight(1), padding(16, 0, 16, clearance + 24)]}
            verticalArrangement={{ spacedBy: 20 }}
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
                          ? { value: row.value }
                          : row.onPress
                            ? "chevron"
                            : "none"
                      }
                    />
                  ),
                )}
              </SettingsSectionItem>
            ))}
          </LazyColumn>
        </Column>
      </Host>
    </SafeAreaView>
  );
}
