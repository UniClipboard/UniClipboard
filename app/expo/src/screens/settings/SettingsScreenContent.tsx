import { useState } from "react";
import { useNavigation } from "@react-navigation/native";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { Modal, Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";
import type { RootStackParamList, SettingsSubSection } from "@/navigation/AppNavigator.types";
import { useShellSettingGroups, type ShellSettingPage } from "@/shell/settings";
import { SettingsPage } from "./SettingsPage";
import { useLiveSettingGroups, type SettingEdit } from "./liveSettings";

type Page = SettingsSubSection | "root";

export function SettingsScreenContent({ page, onBack }: { page: Page; onBack?: () => void }) {
  const { t } = useTranslation();
  const mode = useEngineStore((state) => state.mode);
  const title = page === "root"
    ? t("common:action.settings")
    : page === "clipboard"
      ? t("settings:ios.extensions.clipboardAccess")
      : t(`settings:category.${page}`, { defaultValue: page });
  if (mode === "unavailable") {
    return <DemoSettings page={page} title={title} onBack={onBack} />;
  }
  return <LiveSettings page={page} title={title} onBack={onBack} />;
}

function DemoSettings({ page, title, onBack }: { page: Page; title: string; onBack?: () => void }) {
  const groups = useShellSettingGroups(page as ShellSettingPage | "root");
  return <SettingsPage title={title} groups={groups} onBack={onBack} />;
}

function LiveSettings({ page, title, onBack }: { page: Page; title: string; onBack?: () => void }) {
  const [edit, setEdit] = useState<SettingEdit | null>(null);
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const groups = useLiveSettingGroups(page, setEdit, (section) => navigation.push("SettingsSub", { section }));
  return (
    <>
      <SettingsPage title={title} groups={groups} onBack={onBack} />
      {edit && <SettingEditor key={edit.id} edit={edit} onClose={() => setEdit(null)} />}
    </>
  );
}

function SettingEditor({ edit, onClose }: { edit: SettingEdit; onClose: () => void }) {
  const { t } = useTranslation();
  const { theme: { colors } } = useTheme();
  const [value, setValue] = useState(edit.value);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const save = async () => {
    if (saving) return;
    setSaving(true);
    try {
      await edit.save(value);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("settingsStorage:setFailed"));
      setSaving(false);
    }
  };

  return (
    <Modal transparent animationType="fade" onRequestClose={saving ? undefined : onClose}>
      <View style={styles.backdrop}>
        <View accessibilityViewIsModal style={[styles.dialog, { backgroundColor: colors.surface }]}>
          <Text accessibilityRole="header" style={[styles.title, { color: colors.textPrimary }]}>{edit.title}</Text>
          <TextInput
            testID={`${edit.id}-input`}
            accessibilityLabel={edit.title}
            value={value}
            onChangeText={setValue}
            autoFocus
            editable={!saving}
            autoCorrect={false}
            autoCapitalize="none"
            keyboardType={edit.numeric ? "number-pad" : "default"}
            onSubmitEditing={() => void save()}
            style={[styles.input, { backgroundColor: colors.surfaceHighest, color: colors.textPrimary }]}
          />
          {!!error && <Text accessibilityRole="alert" style={{ color: colors.error }}>{error}</Text>}
          <View style={styles.buttons}>
            <Pressable accessibilityRole="button" disabled={saving} onPress={onClose} style={styles.button}>
              <Text style={{ color: colors.textSecondary }}>{t("common:action.cancel")}</Text>
            </Pressable>
            <Pressable testID="setting-save" accessibilityRole="button" disabled={saving} onPress={() => void save()} style={styles.button}>
              <Text style={{ color: colors.accent }}>{t("common:action.save")}</Text>
            </Pressable>
          </View>
        </View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: { flex: 1, justifyContent: "center", alignItems: "center", padding: 24, backgroundColor: "rgba(0,0,0,0.45)" },
  dialog: { width: "100%", maxWidth: 420, borderRadius: 20, padding: 20, gap: 12 },
  title: { fontSize: 19, fontWeight: "600" },
  input: { paddingHorizontal: 12, minHeight: 46, borderRadius: 8, fontSize: 17 },
  buttons: { flexDirection: "row", justifyContent: "flex-end" },
  button: { padding: 12, minHeight: 44, justifyContent: "center" },
});
