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
import type { SFSymbol } from "sf-symbols-typescript";
import { iosAccentColor } from "@/theme/iosDesignTokens";
import type { SettingsPageProps } from "./SettingsPage.types";

const ROW_ICONS: Record<string, { icon: SFSymbol; color: string }> = {
  "auto-apply": { icon: "arrow.down.doc", color: settingsTileColors.green },
  "auto-push": { icon: "arrow.up.doc", color: settingsTileColors.teal },
  sync: { icon: "arrow.triangle.2.circlepath", color: settingsTileColors.blue },
  history: { icon: "clock.arrow.circlepath", color: settingsTileColors.blue },
  clipboard: { icon: "doc.on.clipboard", color: settingsTileColors.orange },
  appearance: { icon: "circle.lefthalf.filled", color: settingsTileColors.indigo },
  language: { icon: "globe", color: settingsTileColors.blue },
  storage: { icon: "externaldrive", color: settingsTileColors.purple },
  network: { icon: "network", color: settingsTileColors.teal },
  background: { icon: "bolt.fill", color: settingsTileColors.orange },
  keyboard: { icon: "keyboard", color: settingsTileColors.indigo },
  share: { icon: "square.and.arrow.up", color: settingsTileColors.green },
  diagnostics: { icon: "waveform.path.ecg", color: settingsTileColors.red },
  privacy: { icon: "hand.raised.fill", color: settingsTileColors.blue },
  about: { icon: "info.circle", color: settingsTileColors.gray },
  developer: { icon: "wrench.and.screwdriver", color: settingsTileColors.indigo },
};

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
                  {group.rows.map((row) => {
                    const iconSpec = ROW_ICONS[row.id];
                    return row.toggle ? (
                      <IconToggleRow
                        key={row.id}
                        testID={row.id}
                        icon={iconSpec?.icon ?? "arrow.triangle.2.circlepath"}
                        iconColor={iconSpec?.color ?? settingsTileColors.green}
                        label={row.title}
                        description={row.description}
                        isOn={row.toggle.value}
                        onIsOnChange={row.toggle.onChange}
                      />
                    ) : row.choices ? (
                      <SettingsPickerRow
                        key={row.id}
                        testID={row.id}
                        icon={iconSpec?.icon ?? "slider.horizontal.3"}
                        iconColor={iconSpec?.color ?? settingsTileColors.indigo}
                        title={row.title}
                        selection={row.choices.value}
                        options={row.choices.options}
                        onSelectionChange={row.choices.onChange}
                      />
                    ) : (
                      <SettingsNavRow
                        key={row.id}
                        testID={row.id}
                        icon={iconSpec?.icon ?? "gearshape"}
                        iconColor={iconSpec?.color ?? settingsTileColors.blue}
                        title={row.title}
                        subtitle={row.description}
                        value={row.value}
                        onPress={row.onPress}
                        readOnly={!row.onPress}
                        showsChevron={!!row.onPress}
                      />
                    );
                  })}
                </Section>
              ))}
            </IosSheetForm>
          </IosSheetPage>
        </IosPageChromeProvider>
      </NavigationStack>
    </Host>
  );
}
