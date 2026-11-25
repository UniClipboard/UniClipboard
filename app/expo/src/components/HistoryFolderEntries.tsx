import { useEffect, useState } from "react";
import { StyleSheet, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { unicEngineNative } from "@/engine/nativeModule";
import type { HistoryFolder } from "@/engine/types";
import { useTheme } from "@/hooks/useTheme";
import type { ClipboardItem } from "@/types/clipboard";
import { formatFileSize } from "@/utils";

export function HistoryFolderEntries({ item }: { item: ClipboardItem }) {
  const { t } = useTranslation("history");
  const { theme: { colors } } = useTheme();
  const [result, setResult] = useState<{ id: string; folder?: HistoryFolder; failed?: boolean }>();
  const id = item.profileHash;

  useEffect(() => {
    let active = true;
    void unicEngineNative.listHistoryFolder(id).then(
      (folder) => { if (active) setResult({ id, folder }); },
      () => { if (active) setResult({ id, failed: true }); },
    );
    return () => { active = false; };
  }, [id]);

  const current = result?.id === id ? result : undefined;
  const folder = current?.folder;
  return (
    <View style={[styles.container, { backgroundColor: colors.surface }]}>
      <Text accessibilityRole="header" style={[styles.heading, { color: colors.textPrimary }]}>
        {t("media.folderContents")}
      </Text>
      {!folder && <Text style={{ color: colors.textSecondary }}>
        {t(current?.failed ? "media.folderUnavailable" : "media.folderLoading")}
      </Text>}
      {folder?.entries.map((entry) => (
        <View key={entry.path} style={styles.row}>
          <Text selectable style={[styles.path, { color: colors.textPrimary }]}>
            {entry.path}{entry.directory ? "/" : ""}
          </Text>
          {!entry.directory && <Text style={{ color: colors.textSecondary }}>{formatFileSize(entry.size)}</Text>}
        </View>
      ))}
      {folder?.entries.length === 0 && <Text style={{ color: colors.textSecondary }}>{t("media.folderEmpty")}</Text>}
      {folder?.truncated && <Text style={{ color: colors.textSecondary }}>{t("media.folderTruncated")}</Text>}
    </View>
  );
}

const styles = StyleSheet.create({
  container: { borderRadius: 16, padding: 16, gap: 10 },
  heading: { fontSize: 17, fontWeight: "600" },
  row: { flexDirection: "row", alignItems: "flex-start", gap: 12 },
  path: { flex: 1 },
});
