import { useMemo, useState } from "react";
import { Alert, Pressable, ScrollView, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { AppBottomSheet, AppButton, AppHost } from "@/components/ui";
import { buildDeviceNameMap } from "@/engine/mapper";
import { useEngineStore } from "@/engine/store";
import type { ReceivedContent } from "@/engine/types";
import { useTheme } from "@/hooks/useTheme";
import { formatFileSize } from "@/utils";
import { formatRelativeTime } from "@/utils/displayKind";

function describeReceivedContent(entry: ReceivedContent): string {
  const parts = entry.content?.parts ?? [];
  const fileParts = parts.filter((p) => p.kind === "file");
  if (fileParts.length > 1) {
    return `${fileParts.length} 个文件`;
  }
  if (fileParts.length === 1) {
    return fileParts[0].name || "文件";
  }
  if (parts.some((p) => p.mime?.startsWith("image/"))) {
    return "图片内容";
  }
  if (parts.some((p) => p.mime === "text/plain" || p.mime === "text/html")) {
    return "文本内容";
  }
  return "剪贴板内容";
}

function applyStateLabel(state: string): string {
  switch (state) {
    case "applied":
      return "已复制到剪贴板";
    case "applying":
      return "应用中…";
    case "failed":
      return "应用失败";
    case "deferred":
      return "已延迟";
    default:
      return "待应用";
  }
}

export function ReceiveResultsSheet({
  visible,
  onClose,
}: {
  visible: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const received = useEngineStore((s) => s.received);
  const devices = useEngineStore((s) => s.devices);
  const autoApplyReceived = useEngineStore((s) => s.autoApplyReceived);
  const [busyId, setBusyId] = useState<string | null>(null);

  const deviceNames = useMemo(() => buildDeviceNameMap(devices), [devices]);

  const handleApply = async (entry: ReceivedContent) => {
    if (busyId) return;
    setBusyId(entry.receipt_id);
    try {
      await useEngineStore.getState().applyReceivedToClipboard(entry.receipt_id);
    } catch (err) {
      Alert.alert(
        t("common:action.copy"),
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    } finally {
      setBusyId(null);
    }
  };

  const handleDismiss = async (entry: ReceivedContent) => {
    if (busyId) return;
    setBusyId(entry.receipt_id);
    try {
      await useEngineStore.getState().dismissReceived(entry.receipt_id);
    } catch (err) {
      Alert.alert(
        t("common:action.close"),
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    } finally {
      setBusyId(null);
    }
  };

  return (
    <AppBottomSheet visible={visible} onDismiss={onClose}>
      <View style={{ padding: 16, gap: 12, maxHeight: 540 }}>
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
          <Text
            accessibilityRole="header"
            style={{
              color: theme.colors.textPrimary,
              fontSize: 20,
              fontWeight: "700",
            }}
          >
            远端接收队列
          </Text>
          <Pressable
            onPress={() => {
              void useEngineStore.getState().setAutoApplyReceived(!autoApplyReceived).catch((error) => {
                Alert.alert(t("settingsStorage:setFailed"), error instanceof Error ? error.message : undefined);
              });
            }}
            style={{
              paddingHorizontal: 10,
              paddingVertical: 6,
              borderRadius: 12,
              backgroundColor: autoApplyReceived
                ? theme.colors.accentContainer
                : theme.colors.surface,
            }}
          >
            <Text
              style={{
                color: autoApplyReceived
                  ? theme.colors.onAccentContainer
                  : theme.colors.textSecondary,
                fontSize: 12,
                fontWeight: "600",
              }}
            >
              {autoApplyReceived ? "自动应用：开" : "自动应用：关"}
            </Text>
          </Pressable>
        </View>

        {received.length === 0 ? (
          <Text style={{ color: theme.colors.textSecondary, paddingVertical: 16 }}>
            当前没有待应用的远端内容。
          </Text>
        ) : (
          <ScrollView style={{ maxHeight: 360 }} contentContainerStyle={{ gap: 10 }}>
            {received.map((entry) => {
              const sender = entry.peer_id
                ? deviceNames.get(entry.peer_id) ?? entry.peer_id.slice(0, 8)
                : entry.channel_id || "远端设备";
              const isBusy = busyId === entry.receipt_id;
              return (
                <View
                  key={entry.receipt_id}
                  style={{
                    padding: 14,
                    borderRadius: 14,
                    backgroundColor: theme.colors.surface,
                    gap: 8,
                  }}
                >
                  <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8 }}>
                    <Text
                      style={{
                        color: theme.colors.textPrimary,
                        fontWeight: "600",
                        fontSize: 15,
                        flex: 1,
                      }}
                      numberOfLines={1}
                    >
                      {describeReceivedContent(entry)}
                    </Text>
                    <Text style={{ color: theme.colors.textSecondary, fontSize: 12 }}>
                      {applyStateLabel(entry.apply_state)}
                    </Text>
                  </View>
                  <Text style={{ color: theme.colors.textSecondary, fontSize: 12 }}>
                    {`来自 ${sender} · ${formatRelativeTime(entry.received_at_ms)}`}
                    {entry.content?.total_size ? ` · ${formatFileSize(entry.content.total_size)}` : ""}
                  </Text>
                  <View style={{ flexDirection: "row", gap: 8, marginTop: 2 }}>
                    <View style={{ flex: 1 }}>
                      <AppHost matchContents={{ vertical: true }}>
                        <AppButton
                          title="复制到剪贴板"
                          variant="filled"
                          fullWidth
                          disabled={isBusy}
                          onPress={() => void handleApply(entry)}
                        />
                      </AppHost>
                    </View>
                    <View style={{ flex: 1 }}>
                      <AppHost matchContents={{ vertical: true }}>
                        <AppButton
                          title="忽略"
                          variant="outlined"
                          fullWidth
                          disabled={isBusy}
                          onPress={() => void handleDismiss(entry)}
                        />
                      </AppHost>
                    </View>
                  </View>
                </View>
              );
            })}
          </ScrollView>
        )}

        <AppHost matchContents={{ vertical: true }}>
          <AppButton
            testID="receive-results-close"
            title={t("common:action.close")}
            variant="text"
            fullWidth
            onPress={onClose}
          />
        </AppHost>
      </View>
    </AppBottomSheet>
  );
}
