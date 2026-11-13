import {
  Button,
  Host,
  NavigationStack,
  Section,
  Text as SwiftUIText,
} from "@expo/ui/swift-ui";
import { frame, tint } from "@expo/ui/swift-ui/modifiers";
import { useTranslation } from "react-i18next";
import {
  IosPageChromeProvider,
  IosSheetForm,
  IosSheetPage,
} from "@/components/ui";
import {
  IconToggleRow,
  SettingsNavRow,
  SettingsPickerRow,
  settingsTileColors,
} from "@/screens/settings/ios/common";
import { iosAccentColor } from "@/theme/iosDesignTokens";
import type { SettingsPageProps } from "./SettingsPage.types";

export function SettingsPage({ title, groups, onBack }: SettingsPageProps) {
  const { t } = useTranslation();
  return (
    <Host style={{ flex: 1 }}>
      <NavigationStack
        modifiers={[
          frame({ maxWidth: Infinity, maxHeight: Infinity }),
          ...(iosAccentColor ? [tint(iosAccentColor)] : []),
        ]}
      >
        <IosPageChromeProvider
          value={{ kind: onBack ? "sheet" : "navigation" }}
        >
          <IosSheetPage
            title={title}
            left={
              onBack ? (
                <Button
                  testID="settings-back"
                  systemImage="chevron.left"
                  label={t("common:action.back")}
                  onPress={onBack}
                />
              ) : undefined
            }
          >
            <IosSheetForm>
              {groups.map((group, index) => (
                <Section
                  key={index}
                  header={
                    group.title ? (
                      <SwiftUIText>{group.title}</SwiftUIText>
                    ) : undefined
                  }
                  footer={
                    group.footer ? (
                      <SwiftUIText>{group.footer}</SwiftUIText>
                    ) : undefined
                  }
                >
                  {group.rows.map((row) =>
                    row.toggle ? (
                      <IconToggleRow
                        key={row.id}
                        testID={row.id}
                        icon="arrow.triangle.2.circlepath"
                        iconColor={settingsTileColors.green}
                        label={row.title}
                        description={row.description}
                        isOn={row.toggle.value}
                        onIsOnChange={row.toggle.onChange}
                      />
                    ) : row.choices ? (
                      <SettingsPickerRow
                        key={row.id}
                        testID={row.id}
                        icon="slider.horizontal.3"
                        iconColor={settingsTileColors.indigo}
                        title={row.title}
                        selection={row.choices.value}
                        options={row.choices.options}
                        onSelectionChange={row.choices.onChange}
                      />
                    ) : (
                      <SettingsNavRow
                        key={row.id}
                        testID={row.id}
                        icon="circle"
                        iconColor={settingsTileColors.blue}
                        title={row.title}
                        value={row.value}
                        onPress={row.onPress}
                        readOnly={!row.onPress}
                        showsChevron={!!row.onPress}
                      />
                    ),
                  )}
                </Section>
              ))}
            </IosSheetForm>
          </IosSheetPage>
        </IosPageChromeProvider>
      </NavigationStack>
    </Host>
  );
}
