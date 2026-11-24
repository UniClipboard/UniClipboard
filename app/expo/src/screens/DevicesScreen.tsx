import { useState } from "react";
import { Alert, Text, TextInput, View } from "react-native";
import { useNavigation } from "@react-navigation/native";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { useTranslation } from "react-i18next";
import {
  AppAlertDialog,
  AppBottomSheet,
  AppButton,
  AppHost,
} from "@/components/ui";
import { useEngineStore } from "@/engine/store";
import type { EngineDevice } from "@/engine/types";
import type {
  RootStackParamList,
  SpaceDeviceTarget,
} from "@/navigation/AppNavigator.types";
import { useTheme } from "@/hooks/useTheme";
import { SettingsPage } from "@/shell/SettingsPage";
import { showUnavailable } from "@/shell/notice";

export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const navigation =
    useNavigation<NativeStackNavigationProp<RootStackParamList>>();

  const mode = useEngineStore((s) => s.mode);
  const devices = useEngineStore((s) => s.devices);
  const candidates = useEngineStore((s) => s.candidates);
  const pairingCode = useEngineStore((s) => s.pairingCode);

  const [joinSheetOpen, setJoinSheetOpen] = useState(false);
  const [joinInput, setJoinInput] = useState("");
  const [joining, setJoining] = useState(false);
  const [ticketSheetOpen, setTicketSheetOpen] = useState(false);
  const [exportedTicketText, setExportedTicketText] = useState("");
  const [pendingApproveDevice, setPendingApproveDevice] =
    useState<EngineDevice | null>(null);
  const [selectedTrustedDevice, setSelectedTrustedDevice] =
    useState<EngineDevice | null>(null);

  if (mode !== "live") {
    return (
      <SettingsPage
        title={t("home:nav.devices")}
        groups={[
          {
            title: t("settings:syncChannel.title"),
            footer: t("shell:unavailable"),
            rows: [
              {
                id: "devices-add",
                title: t("shell:addConnection"),
                onPress: () => navigation.navigate("ConnectionPreview"),
              },
            ],
          },
          {
            title: t("shell:demoDevices"),
            rows: ["Demo Mac", "Demo Phone"].map((name, index) => ({
              id: `device-${index}`,
              title: name,
              value: t("shell:disconnected"),
              onPress: showUnavailable,
            })),
          },
        ]}
      />
    );
  }

  const pendingDevices = devices.filter((d) => d.trust === "pending");
  const trustedDevices = devices.filter((d) => d.trust === "trusted");

  const handleGenerateCode = async () => {
    try {
      await useEngineStore.getState().createPairingCode();
    } catch (err) {
      Alert.alert(
        "配对码",
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    }
  };

  const handleExportTicket = async () => {
    try {
      const res = await useEngineStore.getState().exportPairingTicket();
      setExportedTicketText(res.ticket);
      setTicketSheetOpen(true);
    } catch (err) {
      Alert.alert(
        "配对票据",
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    }
  };

  const handleJoinSubmit = async () => {
    const raw = joinInput.trim();
    if (!raw || joining) return;
    setJoining(true);
    try {
      const normalizedDigits = raw.replace(/\s+/g, "");
      if (/^\d{3}-?\d{3}$/.test(normalizedDigits)) {
        await useEngineStore.getState().joinByCode(normalizedDigits);
      } else {
        await useEngineStore.getState().joinByTicket(raw);
      }
      setJoinInput("");
      setJoinSheetOpen(false);
    } catch (err) {
      Alert.alert(
        "加入设备失败",
        err instanceof Error ? err.message : t("shell:unavailable"),
      );
    } finally {
      setJoining(false);
    }
  };

  return (
    <>
      <SettingsPage
        title={t("home:nav.devices")}
        groups={[
          {
            title: "配对新设备",
            footer: pairingCode
              ? `当前配对码：${pairingCode.code}（请在另一台设备输入后核对指纹并批准）`
              : "支持 6 位一次性配对码或离线票据配对，经人工核对指纹后生效。",
            rows: [
              {
                id: "devices-create-code",
                title: pairingCode ? `配对码：${pairingCode.code}` : "生成 6 位配对码",
                description: pairingCode ? "点击重新生成配对码" : "供其他设备输入连接",
                onPress: () => void handleGenerateCode(),
              },
              ...(pairingCode
                ? [
                    {
                      id: "devices-cancel-code",
                      title: "取消当前配对码",
                      onPress: () =>
                        void useEngineStore.getState().cancelPairingCode(),
                    },
                  ]
                : []),
              {
                id: "devices-join",
                title: "输入配对码或票据",
                description: "连接到已生成配对码的另一台设备",
                onPress: () => setJoinSheetOpen(true),
              },
              {
                id: "devices-export-ticket",
                title: "导出配对票据",
                description: "生成一次性完整连接票据（敏感信息，妥善保管）",
                onPress: () => void handleExportTicket(),
              },
            ],
          },
          ...(pendingDevices.length > 0
            ? [
                {
                  title: "待审批设备（需核对指纹）",
                  rows: pendingDevices.map((dev) => ({
                    id: `pending-${dev.id}`,
                    title: dev.name || dev.reported_name || dev.id.slice(0, 8),
                    description: `指纹：${dev.fingerprint}`,
                    value: "去审批",
                    onPress: () => setPendingApproveDevice(dev),
                  })),
                },
              ]
            : []),
          {
            title: "已配对设备",
            footer:
              trustedDevices.length === 0
                ? "暂未配对设备；完成配对并核对指纹后即可同步。"
                : undefined,
            rows:
              trustedDevices.length > 0
                ? trustedDevices.map((dev) => {
                    const primaryBinding = dev.bindings?.[0];
                    const statusText = !primaryBinding
                      ? "无绑定路径"
                      : primaryBinding.connection === "connected"
                        ? primaryBinding.sync_enabled
                          ? "已连接"
                          : "已连接（已暂停同步）"
                        : primaryBinding.connection === "connecting"
                          ? "连接中…"
                          : "离线";
                    return {
                      id: `device-${dev.id}`,
                      title: dev.name || dev.reported_name || dev.id.slice(0, 8),
                      description: `指纹：${dev.fingerprint}`,
                      value: statusText,
                      onPress: () => setSelectedTrustedDevice(dev),
                    };
                  })
                : [
                    {
                      id: "device-empty",
                      title: "暂无已配对设备",
                    },
                  ],
          },
          ...(candidates.length > 0
            ? [
                {
                  title: "发现的推荐设备",
                  rows: candidates.map((c) => ({
                    id: `candidate-${c.device_id}`,
                    title: c.name || c.device_id.slice(0, 8),
                    description: `由 ${c.introducers.map((i) => i.name).join("、") || "已信任设备"} 推荐`,
                    value: c.requested ? "配对中" : "发起配对",
                    onPress: () =>
                      void useEngineStore.getState().pairCandidate(c),
                  })),
                },
              ]
            : []),
        ]}
      />

      <AppAlertDialog
        visible={pendingApproveDevice !== null}
        standalone
        title="核对设备指纹并批准？"
        message={
          pendingApproveDevice
            ? `设备名称：${pendingApproveDevice.reported_name || pendingApproveDevice.id}\n\n安全指纹：\n${pendingApproveDevice.fingerprint}\n\n请确认对端屏幕显示的指纹完全一致后再信任并允许同步。`
            : ""
        }
        confirmLabel="信任并授权同步"
        onConfirm={() => {
          const target = pendingApproveDevice;
          setPendingApproveDevice(null);
          if (target) {
            void useEngineStore
              .getState()
              .approveDevice(target, target.fingerprint)
              .catch((err) => {
                Alert.alert(
                  "审批失败",
                  err instanceof Error ? err.message : t("shell:unavailable"),
                );
              });
          }
        }}
        dismissLabel={t("common:action.cancel")}
        onDismiss={() => setPendingApproveDevice(null)}
        onDismissAction={() => setPendingApproveDevice(null)}
      />

      <AppBottomSheet
        visible={joinSheetOpen}
        onDismiss={() => setJoinSheetOpen(false)}
      >
        <View style={{ padding: 16, gap: 12 }}>
          <Text
            style={{
              color: theme.colors.textPrimary,
              fontSize: 18,
              fontWeight: "700",
            }}
          >
            配对另一台设备
          </Text>
          <TextInput
            testID="devices-join-input"
            value={joinInput}
            onChangeText={setJoinInput}
            placeholder="输入 6 位配对码（如 123-456）或粘贴票据"
            placeholderTextColor={theme.colors.textSecondary}
            autoCapitalize="none"
            autoCorrect={false}
            style={{
              borderRadius: 12,
              padding: 12,
              backgroundColor: theme.colors.surface,
              color: theme.colors.textPrimary,
            }}
          />
          <AppHost matchContents={{ vertical: true }}>
            <AppButton
              testID="devices-join-submit"
              title="连接设备"
              variant="filled"
              fullWidth
              disabled={joining || !joinInput.trim()}
              onPress={() => void handleJoinSubmit()}
            />
          </AppHost>
          <AppHost matchContents={{ vertical: true }}>
            <AppButton
              title={t("common:action.cancel")}
              variant="text"
              fullWidth
              onPress={() => setJoinSheetOpen(false)}
            />
          </AppHost>
        </View>
      </AppBottomSheet>

      <AppBottomSheet
        visible={ticketSheetOpen}
        onDismiss={() => {
          setTicketSheetOpen(false);
          setExportedTicketText("");
        }}
      >
        <View style={{ padding: 16, gap: 12 }}>
          <Text
            style={{
              color: theme.colors.textPrimary,
              fontSize: 18,
              fontWeight: "700",
            }}
          >
            一次性配对票据
          </Text>
          <Text
            selectable
            style={{
              color: theme.colors.textPrimary,
              backgroundColor: theme.colors.surface,
              padding: 12,
              borderRadius: 12,
              fontSize: 12,
            }}
          >
            {exportedTicketText}
          </Text>
          <AppHost matchContents={{ vertical: true }}>
            <AppButton
              title={t("common:action.close")}
              variant="outlined"
              fullWidth
              onPress={() => {
                setTicketSheetOpen(false);
                setExportedTicketText("");
              }}
            />
          </AppHost>
        </View>
      </AppBottomSheet>

      <AppBottomSheet
        visible={selectedTrustedDevice !== null}
        onDismiss={() => setSelectedTrustedDevice(null)}
      >
        {selectedTrustedDevice ? (
          <View style={{ padding: 16, gap: 12 }}>
            <Text
              style={{
                color: theme.colors.textPrimary,
                fontSize: 18,
                fontWeight: "700",
              }}
            >
              {selectedTrustedDevice.name ||
                selectedTrustedDevice.reported_name ||
                selectedTrustedDevice.id.slice(0, 8)}
            </Text>
            <Text style={{ color: theme.colors.textSecondary, fontSize: 13 }}>
              {`指纹：${selectedTrustedDevice.fingerprint}`}
            </Text>
            {selectedTrustedDevice.bindings?.map((b) => (
              <AppHost key={b.id} matchContents={{ vertical: true }}>
                <AppButton
                  title={b.sync_enabled ? "暂停此设备自动同步" : "恢复此设备自动同步"}
                  variant="tonal"
                  fullWidth
                  onPress={() => {
                    void useEngineStore
                      .getState()
                      .setBindingSync(b.id, b.revision, !b.sync_enabled);
                    setSelectedTrustedDevice(null);
                  }}
                />
              </AppHost>
            ))}
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title="取消配对并撤销信任"
                variant="outlined"
                fullWidth
                onPress={() => {
                  const target = selectedTrustedDevice;
                  setSelectedTrustedDevice(null);
                  void useEngineStore
                    .getState()
                    .unpairDevice(target.id, target.revision);
                }}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={t("common:action.close")}
                variant="text"
                fullWidth
                onPress={() => setSelectedTrustedDevice(null)}
              />
            </AppHost>
          </View>
        ) : null}
      </AppBottomSheet>
    </>
  );
}
