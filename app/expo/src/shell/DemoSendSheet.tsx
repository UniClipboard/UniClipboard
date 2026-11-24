import { useState } from "react";
import { Alert, Pressable, Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { AppBottomSheet, AppButton, AppHost } from "@/components/ui";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";
import { showUnavailable } from "./notice";

export function DemoSendSheet({
  visible,
  onClose,
}: {
  visible: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const mode = useEngineStore((s) => s.mode);
  const devices = useEngineStore((s) => s.devices);
  const [draftText, setDraftText] = useState("");
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  if (mode !== "live") {
    return (
      <AppBottomSheet visible={visible} onDismiss={onClose}>
        <View style={{ padding: 16, gap: 16 }}>
          <Text
            accessibilityRole="header"
            style={{
              color: theme.colors.textPrimary,
              fontSize: 22,
              fontWeight: "600",
            }}
          >
            {t("home:detail.sendTo")}
          </Text>
          <Text style={{ color: theme.colors.textSecondary }}>
            {t("shell:unavailable")}
          </Text>
          {["Demo Mac", "Demo Phone"].map((name) => (
            <Pressable
              key={name}
              testID={`send-${name}`}
              onPress={showUnavailable}
              style={{
                padding: 20,
                borderRadius: 16,
                backgroundColor: theme.colors.surface,
              }}
            >
              <Text style={{ color: theme.colors.textPrimary }}>{name}</Text>
              <Text style={{ color: theme.colors.textSecondary }}>
                {t("shell:disconnected")}
              </Text>
            </Pressable>
          ))}
          <Pressable
            testID="send-close"
            onPress={onClose}
            style={{ padding: 16 }}
          >
            <Text style={{ color: theme.colors.accent }}>
              {t("common:action.close")}
            </Text>
          </Pressable>
        </View>
      </AppBottomSheet>
    );
  }

  const trustedDevices = devices.filter((d) => d.trust === "trusted");
  const targetIds = selectedDeviceId ? [selectedDeviceId] : undefined;

  const handleCommitText = async () => {
    const trimmed = draftText.trim();
    if (!trimmed || submitting) return;
    setSubmitting(true);
    try {
      await useEngineStore.getState().commitText(trimmed, targetIds);
      setDraftText("");
      onClose();
    } catch (err) {
      Alert.alert(
        t("home:detail.sendTo"),
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    } finally {
      setSubmitting(false);
    }
  };

  const handleReadClipboard = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      const res = await useEngineStore.getState().readSystemClipboardAndCommit();
      if (res.skipped) {
        Alert.alert("剪贴板", "当前系统剪贴板为空或已同步。");
      } else {
        onClose();
      }
    } catch (err) {
      Alert.alert(
        "读取系统剪贴板",
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    } finally {
      setSubmitting(false);
    }
  };

  const handlePickFile = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      const res = await useEngineStore.getState().pickAndCommitFile(targetIds);
      if (!res.cancelled) {
        onClose();
      }
    } catch (err) {
      Alert.alert(
        "发送文件",
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AppBottomSheet visible={visible} onDismiss={onClose}>
      <View style={{ padding: 16, gap: 12 }}>
        <Text
          accessibilityRole="header"
          style={{
            color: theme.colors.textPrimary,
            fontSize: 20,
            fontWeight: "700",
          }}
        >
          发送到本机 / 对端设备
        </Text>

        <TextInput
          testID="send-compose-input"
          value={draftText}
          onChangeText={setDraftText}
          placeholder="输入要保存或发送的文本内容…"
          placeholderTextColor={theme.colors.textSecondary}
          multiline
          style={{
            minHeight: 84,
            borderRadius: 14,
            padding: 12,
            backgroundColor: theme.colors.surface,
            color: theme.colors.textPrimary,
            textAlignVertical: "top",
          }}
        />

        {trustedDevices.length > 0 ? (
          <View style={{ gap: 8 }}>
            <Text style={{ color: theme.colors.textSecondary, fontSize: 13 }}>
              目标设备（不选则保存到历史并按同步规则分发）
            </Text>
            {trustedDevices.map((dev) => {
              const selected = selectedDeviceId === dev.id;
              const connected = dev.bindings?.some((b) => b.connection === "connected");
              return (
                <Pressable
                  key={dev.id}
                  onPress={() => setSelectedDeviceId(selected ? null : dev.id)}
                  style={{
                    padding: 14,
                    borderRadius: 14,
                    backgroundColor: selected
                      ? theme.colors.accentContainer
                      : theme.colors.surface,
                  }}
                >
                  <Text style={{ color: theme.colors.textPrimary, fontWeight: "600" }}>
                    {dev.name || dev.reported_name || dev.id.slice(0, 8)}
                  </Text>
                  <Text style={{ color: theme.colors.textSecondary, fontSize: 12 }}>
                    {connected ? "已连接" : "离线 / 待连接"}
                  </Text>
                </Pressable>
              );
            })}
          </View>
        ) : (
          <Text style={{ color: theme.colors.textSecondary, fontSize: 13 }}>
            暂无已配对的在线设备；内容将先保存到本地加密历史。
          </Text>
        )}

        <AppHost matchContents={{ vertical: true }}>
          <AppButton
            testID="send-commit-text"
            title="保存并发送文本"
            variant="filled"
            fullWidth
            disabled={submitting || !draftText.trim()}
            onPress={() => void handleCommitText()}
          />
        </AppHost>
        <AppHost matchContents={{ vertical: true }}>
          <AppButton
            testID="send-read-clipboard"
            title="读取系统剪贴板"
            variant="tonal"
            fullWidth
            disabled={submitting}
            onPress={() => void handleReadClipboard()}
          />
        </AppHost>
        <AppHost matchContents={{ vertical: true }}>
          <AppButton
            testID="send-pick-file"
            title="选择文件发送"
            variant="outlined"
            fullWidth
            disabled={submitting}
            onPress={() => void handlePickFile()}
          />
        </AppHost>
        <AppHost matchContents={{ vertical: true }}>
          <AppButton
            testID="send-close"
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
