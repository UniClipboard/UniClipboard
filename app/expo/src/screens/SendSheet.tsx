import { useEffect, useState } from "react";
import { Alert, Pressable, ScrollView, Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { AppBottomSheet, AppButton, AppHost } from "@/components/ui";
import { parseEngineError } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";
import type { ClipboardItem } from "@/types/clipboard";
import { formatFileSize } from "@/utils";
import { showUnavailable } from "@/shell/notice";
import { useHistorySend } from "./useHistorySend";
import { useSendDevices } from "./useSendDevices";

export function SendSheet({
  visible,
  onClose,
  onDidDismiss,
  targetItem,
  initialDraftText,
}: {
  visible: boolean;
  onClose: () => void;
  onDidDismiss: () => void;
  targetItem?: ClipboardItem | null;
  initialDraftText?: string | null;
}) {
  const { t } = useTranslation();
  const errorMessage = (error: unknown) => t(`errors:engine.${parseEngineError(error).code}`, {
    defaultValue: t("errors:engine.internal"),
  });
  const { theme } = useTheme();
  const mode = useEngineStore((s) => s.mode);
  const [draftText, setDraftText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const { trustedDevices, selectedDeviceIds, toggleDeviceSelection } = useSendDevices(visible);
  const historySend = useHistorySend(visible, targetItem?.profileHash);

  useEffect(() => {
    if (visible) {
      setDraftText(initialDraftText ?? "");
    }
  }, [visible, initialDraftText, targetItem?.profileHash]);

  if (mode !== "live") {
    return (
      <AppBottomSheet visible={visible} onDismiss={onClose} onDidDismiss={onDidDismiss}>
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
          {["demoMac", "demoPhone"].map((device) => (
            <Pressable
              key={device}
              testID={`send-${device}`}
              onPress={showUnavailable}
              style={{
                padding: 20,
                borderRadius: 16,
                backgroundColor: theme.colors.surface,
              }}
            >
              <Text style={{ color: theme.colors.textPrimary }}>{t(`settingsSync:pairing.${device}`)}</Text>
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

  const targetType = targetItem ? t(`errors:contentType.${targetItem.type.toLowerCase()}`, {
    defaultValue: t("errors:contentType.unknown"),
  }) : "";
  const targetIds = selectedDeviceIds.length > 0 ? selectedDeviceIds : undefined;

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
        errorMessage(err),
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
        Alert.alert(t("share:compose.clipboard"), t("share:compose.emptyClipboard"));
      } else {
        onClose();
      }
    } catch (err) {
      Alert.alert(
        t("share:compose.readClipboard"),
        errorMessage(err),
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
        t("share:compose.sendFile"),
        errorMessage(err),
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <AppBottomSheet visible={visible} onDismiss={onClose} onDidDismiss={onDidDismiss}>
      <View style={{ padding: 16, gap: 12 }}>
        <Text
          accessibilityRole="header"
          style={{
            color: theme.colors.textPrimary,
            fontSize: 20,
            fontWeight: "700",
          }}
        >
          {targetItem ? t("home:detail.sendTo") : t("share:compose.sendTitle")}
        </Text>

        {targetItem ? (
          <View
            style={{
              padding: 12,
              borderRadius: 14,
              backgroundColor: theme.colors.surface,
              gap: 4,
            }}
          >
            <Text
              style={{ color: theme.colors.textPrimary, fontWeight: "600", fontSize: 14 }}
              numberOfLines={3}
            >
              {targetItem.dataName || targetItem.text || `[${targetType}]`}
            </Text>
            <Text style={{ color: theme.colors.textSecondary, fontSize: 12 }}>
              {targetType}
              {targetItem.size ? ` · ${formatFileSize(targetItem.size)}` : ""}
            </Text>
          </View>
        ) : (
          <TextInput
            testID="send-compose-input"
            value={draftText}
            onChangeText={setDraftText}
            placeholder={t("share:compose.textHint")}
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
        )}

        {trustedDevices.length > 0 ? (
          <View style={{ gap: 8 }}>
            <Text style={{ color: theme.colors.textSecondary, fontSize: 13 }}>
              {t(targetItem ? "share:send.selectHint" : "share:compose.targets")}
            </Text>
            <ScrollView style={{ maxHeight: 240 }} contentContainerStyle={{ gap: 8 }}>
              {trustedDevices.map((dev) => {
                const selected = selectedDeviceIds.includes(dev.id);
                const connected = dev.bindings?.some((b) => b.connection === "connected");
                const relayReady = dev.bindings?.some((b) => b.relay_route_ready);
                const phase = targetItem && selected ? historySend.phaseFor(dev.id) : "idle";
                const status = phase === "idle"
                  ? t(connected ? "share:compose.connected" : relayReady ? "share:compose.relayReady" : "share:compose.offline")
                  : t(`share:send.targetState.${phase}`);
                return (
                  <Pressable
                    key={dev.id}
                    accessibilityRole="checkbox"
                    accessibilityState={{ checked: selected }}
                    disabled={submitting || historySend.busy || historySend.done}
                    onPress={() => toggleDeviceSelection(dev.id)}
                    style={{
                      padding: 14,
                      borderRadius: 14,
                      backgroundColor: selected ? theme.colors.accentContainer : theme.colors.surface,
                    }}
                  >
                    <Text style={{ color: theme.colors.textPrimary, fontWeight: "600" }}>
                      {dev.name || dev.reported_name || dev.id.slice(0, 8)}
                      {selected ? " ✓" : ""}
                    </Text>
                    <Text style={{ color: theme.colors.textSecondary, fontSize: 12 }}>{status}</Text>
                  </Pressable>
                );
              })}
            </ScrollView>
          </View>
        ) : (
          <Text style={{ color: theme.colors.textSecondary, fontSize: 13 }}>
            {t(targetItem ? "share:send.noDevices" : "share:compose.noTargets")}
          </Text>
        )}

        {targetItem ? (
          <AppHost matchContents={{ vertical: true }}>
            <AppButton
              testID="send-resend-item"
              title={
                historySend.busy ? historySend.waitingLabel
                  : t(historySend.done ? "share:send.done" : "share:send.sendAll")
              }
              variant="filled"
              fullWidth
              disabled={historySend.busy || (!historySend.done && selectedDeviceIds.length === 0)}
              onPress={() => historySend.done ? onClose() : void historySend.send(selectedDeviceIds)}
            />
          </AppHost>
        ) : (
          <>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                testID="send-commit-text"
                title={t("share:compose.commitText")}
                variant="filled"
                fullWidth
                disabled={submitting || !draftText.trim()}
                onPress={() => void handleCommitText()}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                testID="send-read-clipboard"
                title={t("share:compose.readClipboard")}
                variant="tonal"
                fullWidth
                disabled={submitting}
                onPress={() => void handleReadClipboard()}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                testID="send-pick-file"
                title={t("share:compose.chooseFile")}
                variant="outlined"
                fullWidth
                disabled={submitting}
                onPress={() => void handlePickFile()}
              />
            </AppHost>
          </>
        )}

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
