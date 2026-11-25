import { useState } from "react";
import { Alert, ScrollView, Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { AppBottomSheet, AppButton, AppHost } from "@/components/ui";
import { parseEngineError } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";
import type { ClipboardItem } from "@/types/clipboard";

const reserved = new Set(["favorite", "starred", "pinned"]);

export function HistoryTagEditor({ item, onClose }: { item: ClipboardItem | null; onClose: () => void }) {
  return (
    <AppBottomSheet visible={item !== null} onDismiss={onClose}>
      {item && <HistoryTagFields key={item.profileHash} item={item} onClose={onClose} />}
    </AppBottomSheet>
  );
}

function HistoryTagFields({ item, onClose }: { item: ClipboardItem; onClose: () => void }) {
  const { t } = useTranslation("history");
  const { theme } = useTheme();
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const tag = draft.trim();
  const tags = item.tags?.filter((value) => !reserved.has(value)) ?? [];

  const update = async (value: string, present: boolean) => {
    setSaving(true);
    try {
      await useEngineStore.getState().setHistoryTag(item.profileHash, value, present);
      if (present) setDraft("");
    } catch (error) {
      Alert.alert(t("tags.edit"), t(`errors:engine.${parseEngineError(error).code}`, {
        defaultValue: t("errors:engine.internal"),
      }));
    } finally {
      setSaving(false);
    }
  };

  return (
    <View style={{ padding: 16, gap: 12 }}>
      <Text style={{ color: theme.colors.textPrimary, fontSize: 18, fontWeight: "700" }}>{t("tags.edit")}</Text>
      <TextInput
        testID="history-tag-input"
        accessibilityLabel={t("tags.name")}
        placeholder={t("tags.name")}
        placeholderTextColor={theme.colors.textSecondary}
        value={draft}
        onChangeText={setDraft}
        style={{ padding: 12, borderRadius: 12, backgroundColor: theme.colors.surface, color: theme.colors.textPrimary }}
      />
      <AppHost matchContents={{ vertical: true }}>
        <AppButton testID="history-tag-add" title={t("tags.add")} fullWidth
          disabled={saving || !tag || Array.from(tag).length > 64 || reserved.has(tag) || tags.includes(tag)}
          onPress={() => void update(tag, true)} />
      </AppHost>
      <ScrollView style={{ maxHeight: 220 }} keyboardShouldPersistTaps="handled">
        {tags.map((value) => (
          <AppHost key={value} matchContents={{ vertical: true }}>
            <AppButton title={t("tags.remove", { tag: value })} variant="text" fullWidth
              disabled={saving} onPress={() => void update(value, false)} />
          </AppHost>
        ))}
      </ScrollView>
      <AppHost matchContents={{ vertical: true }}>
        <AppButton title={t("common:action.done")} fullWidth onPress={onClose} />
      </AppHost>
    </View>
  );
}
