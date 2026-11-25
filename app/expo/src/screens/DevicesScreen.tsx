import { useEffect, useRef, useState } from "react";
import { Alert, AppState, Image, Text, TextInput, View } from "react-native";
import { useIsFocused, useNavigation } from "@react-navigation/native";
import type { NativeStackNavigationProp } from "@react-navigation/native-stack";
import { useTranslation } from "react-i18next";
import { AppAlertDialog, AppBottomSheet, AppButton, AppHost } from "@/components/ui";
import { parseEngineError, unicEngineNative } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import type { EngineCandidate, EngineDevice, EngineEvent, EngineOperation, SensitiveTicket } from "@/engine/types";
import { useTheme } from "@/hooks/useTheme";
import type { RootStackParamList, SpaceDeviceTarget } from "@/navigation/AppNavigator.types";
import { SettingsPage } from "@/screens/settings/SettingsPage";
import { showUnavailable } from "@/shell/notice";
import { PairingScanner } from "./PairingScanner";

function deviceName(device: EngineDevice) {
  return device.name || device.reported_name || device.id.slice(0, 8);
}

export function DevicesScreen(_props: SpaceDeviceTarget) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) => t(`settingsSync:pairing.${key}`, values);
  const { theme } = useTheme();
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const focused = useIsFocused();

  const mode = useEngineStore((s) => s.mode);
  const self = useEngineStore((s) => s.snapshot?.self);
  const devices = useEngineStore((s) => s.devices);
  const candidates = useEngineStore((s) => s.candidates);
  const pairingCode = useEngineStore((s) => s.pairingCode);

  const [joinMode, setJoinMode] = useState<"manual" | "scan" | null>(null);
  const [joinInput, setJoinInput] = useState("");
  const [starting, setStarting] = useState(false);
  const startingRef = useRef(false);
  const operationGeneration = useRef(0);
  const [operationID, setOperationID] = useState<string | null>(null);
  const [operation, setOperation] = useState<EngineOperation | null>(null);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [ticket, setTicket] = useState<SensitiveTicket | null>(null);
  const ticketGeneration = useRef(0);
  const exporting = useRef(false);
  const [pendingApproveDevice, setPendingApproveDevice] = useState<EngineDevice | null>(null);
  const [selectedDeviceID, setSelectedDeviceID] = useState<string | null>(null);
  const [selectedCandidate, setSelectedCandidate] = useState<EngineCandidate | null>(null);
  const [name, setName] = useState("");
  const [foreground, setForeground] = useState(AppState.currentState === "active");

  const closeTicket = () => {
    ticketGeneration.current++;
    setTicket(null);
  };

  useEffect(() => {
    const subscription = AppState.addEventListener("change", (state) => {
      const active = state === "active";
      setForeground(active);
      if (!active) closeTicket();
    });
    return () => subscription.remove();
  }, []);

  useEffect(() => {
    if (focused && mode === "live") return;
    setJoinMode(null);
    setJoinInput("");
    closeTicket();
  }, [focused, mode]);

  useEffect(() => {
    if (!ticket) return;
    const timer = setTimeout(closeTicket, Math.max(0, ticket.expires_at_ms - Date.now()));
    return () => clearTimeout(timer);
  }, [ticket]);

  useEffect(() => {
    if (!operationID || mode !== "live") return;
    let request = 0;
    let disposed = false;
    const refresh = async () => {
      const current = ++request;
      try {
        const result = await unicEngineNative.getOperation(operationID);
        if (!disposed && current === request) {
          setOperation(result);
          setOperationError(null);
        }
      } catch (error) {
        if (!disposed && current === request) setOperationError(parseEngineError(error).code);
      }
    };
    const events = unicEngineNative.addEventListener((raw) => {
      let event: EngineEvent;
      try {
        event = JSON.parse(raw) as EngineEvent;
      } catch {
        return;
      }
      if (event.kind === "operations" && event.objects?.some(({ id }) => id === operationID)) {
        void refresh();
      }
    });
    const resync = unicEngineNative.addResyncListener(() => void refresh());
    const active = AppState.addEventListener("change", (state) => {
      if (state === "active") void refresh();
    });
    void refresh();
    return () => {
      disposed = true;
      events.remove();
      resync.remove();
      active.remove();
    };
  }, [operationID, mode]);

  const errorText = (error: unknown) => {
    const code = parseEngineError(error).code;
    return t(`errors:engine.${code}`, { defaultValue: t("errors:engine.internal") });
  };

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (error) {
      Alert.alert(pair("actionFailed"), errorText(error));
    }
  };

  const start = async (action: () => Promise<string>) => {
    if (startingRef.current) return;
    startingRef.current = true;
    const generation = ++operationGeneration.current;
    setStarting(true);
    setOperationID(null);
    setOperation(null);
    setOperationError(null);
    try {
      const id = await action();
      if (generation === operationGeneration.current) setOperationID(id);
    } catch (error) {
      if (generation === operationGeneration.current) Alert.alert(pair("actionFailed"), errorText(error));
    } finally {
      startingRef.current = false;
      setStarting(false);
    }
  };

  const submit = (input: string) => {
    const raw = input.trim();
    const digits = raw.replace(/\s+/g, "");
    const store = useEngineStore.getState();
    setJoinMode(null);
    setJoinInput("");
    if (/^\d{3}-?\d{3}$/.test(digits)) {
      void start(() => store.joinByCode(digits));
    } else if (raw.startsWith("unic-pair:")) {
      void start(() => store.joinByTicket(raw));
    } else {
      Alert.alert(pair("actionFailed"), pair("invalidInput"));
    }
  };

  const exportTicket = async () => {
    if (exporting.current) return;
    exporting.current = true;
    const generation = ++ticketGeneration.current;
    try {
      const result = await useEngineStore.getState().exportPairingTicket();
      if (generation === ticketGeneration.current && result.expires_at_ms > Date.now()) {
        setTicket(result);
      }
    } catch (error) {
      if (generation === ticketGeneration.current) Alert.alert(pair("actionFailed"), errorText(error));
    } finally {
      exporting.current = false;
    }
  };

  if (mode !== "live") {
    return (
      <SettingsPage
        title={t("home:nav.devices")}
        groups={[
          {
            title: t("settings:syncChannel.title"),
            footer: t("shell:unavailable"),
            rows: [{
              id: "devices-add",
              title: t("shell:addConnection"),
              onPress: () => navigation.navigate("ConnectionPreview"),
            }],
          },
          {
            title: t("shell:demoDevices"),
            rows: ["demoMac", "demoPhone"].map((device, index) => ({
              id: `device-${index}`,
              title: pair(device),
              value: t("shell:disconnected"),
              onPress: showUnavailable,
            })),
          },
        ]}
      />
    );
  }

  const store = useEngineStore.getState();
  const pendingDevices = devices.filter((d) => d.trust === "pending");
  const trustedDevices = devices.filter((d) => d.trust === "trusted");
  const trusted = trustedDevices.find((d) => d.id === selectedDeviceID);
  const peer = devices.find((d) => d.id === operation?.pairing?.peer_id);
  const waitingApproval = Boolean(
    operation?.pairing && (
      (operation.pairing.awaiting_local_approval && peer?.trust !== "trusted") ||
      (operation.pairing.awaiting_remote_approval && !peer?.bindings?.some((binding) => binding.connection === "connected"))
    ),
  );
  const phase = waitingApproval ? "waitingApproval" : operation?.phase ?? "resolving";
  const operationMessage = operationError || operation?.error?.code;
  const operationStatus = operationMessage
    ? t(`errors:engine.${operationMessage}`, { defaultValue: t("errors:engine.internal") })
    : t(`settingsSync:pairing.phase.${phase}`, { defaultValue: pair("phase.resolving") });
  const titleStyle = { color: theme.colors.textPrimary, fontSize: 18, fontWeight: "700" as const };
  const inputStyle = {
    borderRadius: 12,
    padding: 12,
    backgroundColor: theme.colors.surface,
    color: theme.colors.textPrimary,
  };
  const closeJoin = () => {
    setJoinMode(null);
    setJoinInput("");
  };

  return (
    <>
      <SettingsPage
        title={t("home:nav.devices")}
        groups={[
          ...(self ? [{
            title: pair("thisDevice"),
            rows: [{ id: "devices-local-identity", title: self.name, description: pair("deviceID", { id: self.id }) }],
          }] : []),
          {
            title: pair("newDevice"),
            footer: pairingCode ? pair("codeInstructions") : pair("instructions"),
            rows: [
              {
                id: "devices-create-code",
                title: pairingCode ? pair("code", { code: pairingCode.code }) : pair("generateCode"),
                onPress: () => void run(store.createPairingCode),
              },
              ...(pairingCode ? [{
                id: "devices-cancel-code",
                title: pair("cancelCode"),
                onPress: () => void run(store.cancelPairingCode),
              }] : []),
              { id: "devices-join", title: pair("enterCode"), onPress: () => setJoinMode("manual") },
              { id: "devices-scan", title: pair("scan"), onPress: () => setJoinMode("scan") },
              { id: "devices-export-ticket", title: pair("showTicket"), onPress: () => void exportTicket() },
            ],
          },
          ...(starting || operationID ? [{
            title: pair("progress"),
            rows: [
              { id: "devices-pairing-status", title: operationStatus },
              {
                id: "devices-stop-waiting",
                title: pair("stopWaiting"),
                description: pair("stopWaitingHelp"),
                onPress: () => {
                  operationGeneration.current++;
                  setStarting(false);
                  setOperationID(null);
                  setOperation(null);
                  setOperationError(null);
                },
              },
            ],
          }] : []),
          ...(pendingDevices.length ? [{
            title: pair("pending"),
            rows: pendingDevices.map((device) => ({
              id: `pending-${device.id}`,
              title: deviceName(device),
              description: pair("fingerprint", { value: device.fingerprint }),
              value: pair("review"),
              onPress: () => setPendingApproveDevice(device),
            })),
          }] : []),
          {
            title: pair("trusted"),
            rows: trustedDevices.length ? trustedDevices.map((device) => {
              const binding = device.bindings?.[0];
              const status = !binding ? "noBinding"
                : binding.connection === "connected" ? (binding.sync_enabled ? "connected" : "paused")
                : binding.connection === "connecting" ? "connecting" : "offline";
              return {
                id: `device-${device.id}`,
                title: deviceName(device),
                description: pair("fingerprint", { value: device.fingerprint }),
                value: pair(status),
                onPress: () => {
                  setName(device.name || device.reported_name || "");
                  setSelectedDeviceID(device.id);
                },
              };
            }) : [{ id: "device-empty", title: pair("empty") }],
          },
          ...(candidates.length ? [{
            title: pair("recommended"),
            rows: candidates.map((candidate) => ({
              id: `candidate-${candidate.device_id}`,
              title: candidate.name || candidate.device_id.slice(0, 8),
              value: pair(candidate.requested ? "progress" : "review"),
              onPress: () => setSelectedCandidate(candidate),
            })),
          }] : []),
        ]}
      />

      <AppAlertDialog
        visible={pendingApproveDevice !== null}
        standalone
        title={pair("approveTitle")}
        message={pendingApproveDevice ? pair("approveBody", {
          name: deviceName(pendingApproveDevice),
          value: pendingApproveDevice.fingerprint,
        }) : ""}
        confirmLabel={pair("approve")}
        onConfirm={() => {
          const target = pendingApproveDevice;
          setPendingApproveDevice(null);
          if (target) void run(() => store.approveDevice(target, target.fingerprint));
        }}
        dismissLabel={t("common:action.cancel")}
        onDismiss={() => setPendingApproveDevice(null)}
        onDismissAction={() => setPendingApproveDevice(null)}
      />

      <AppBottomSheet visible={joinMode !== null} onDismiss={closeJoin}>
        <View style={{ padding: 16, gap: 12 }}>
          <Text style={titleStyle}>{pair(joinMode === "scan" ? "scan" : "enterCode")}</Text>
          {joinMode === "scan" ? (
            focused && foreground && <PairingScanner onRead={submit} />
          ) : (
            <>
              <TextInput
                testID="devices-join-input"
                value={joinInput}
                onChangeText={setJoinInput}
                placeholder={pair("inputHint")}
                placeholderTextColor={theme.colors.textSecondary}
                autoCapitalize="none"
                autoCorrect={false}
                style={inputStyle}
              />
              <AppHost matchContents={{ vertical: true }}>
                <AppButton
                  testID="devices-join-submit"
                  title={pair("connect")}
                  variant="filled"
                  fullWidth
                  disabled={starting || !joinInput.trim()}
                  onPress={() => submit(joinInput)}
                />
              </AppHost>
            </>
          )}
          <AppHost matchContents={{ vertical: true }}>
            <AppButton title={t("common:action.cancel")} variant="text" fullWidth onPress={closeJoin} />
          </AppHost>
        </View>
      </AppBottomSheet>

      <AppBottomSheet visible={ticket !== null && focused && foreground} onDismiss={closeTicket}>
        {ticket && focused && foreground && (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>{pair("ticketTitle")}</Text>
            <Text style={{ color: theme.colors.textPrimary }}>{pair("ticketHelp")}</Text>
            {ticket.qr_png && (
              <Image
                testID="devices-pairing-qr"
                accessibilityLabel={pair("ticketTitle")}
                source={{ uri: `data:image/png;base64,${ticket.qr_png}` }}
                style={{ width: 260, height: 260, alignSelf: "center" }}
              />
            )}
            <AppHost matchContents={{ vertical: true }}>
              <AppButton
                title={t("common:action.copy")}
                variant="filled"
                fullWidth
                onPress={() => void run(async () => {
                  if (ticket.expires_at_ms <= Date.now()) {
                    closeTicket();
                    return;
                  }
                  await unicEngineNative.copyPairingTicket(ticket.ticket, ticket.expires_at_ms);
                  Alert.alert(t("errors:copy.copied"));
                })}
              />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton title={t("common:action.close")} variant="text" fullWidth onPress={closeTicket} />
            </AppHost>
          </View>
        )}
      </AppBottomSheet>

      <AppBottomSheet visible={selectedCandidate !== null} onDismiss={() => setSelectedCandidate(null)}>
        {selectedCandidate && (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>{selectedCandidate.name || selectedCandidate.device_id.slice(0, 8)}</Text>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton title={pair("connect")} fullWidth disabled={starting || selectedCandidate.requested} onPress={() => {
                setSelectedCandidate(null);
                void start(() => store.pairCandidate(selectedCandidate));
              }} />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton title={pair("ignore")} variant="outlined" fullWidth onPress={() => {
                setSelectedCandidate(null);
                void run(() => store.ignoreCandidate(selectedCandidate.device_id, true));
              }} />
            </AppHost>
          </View>
        )}
      </AppBottomSheet>

      <AppBottomSheet visible={trusted !== undefined} onDismiss={() => setSelectedDeviceID(null)}>
        {trusted && (
          <View style={{ padding: 16, gap: 12 }}>
            <Text style={titleStyle}>{deviceName(trusted)}</Text>
            <Text style={{ color: theme.colors.textSecondary }}>{pair("fingerprint", { value: trusted.fingerprint })}</Text>
            <TextInput
              testID="devices-rename-input"
              accessibilityLabel={pair("rename")}
              value={name}
              onChangeText={setName}
              style={inputStyle}
            />
            <AppHost matchContents={{ vertical: true }}>
              <AppButton title={pair("rename")} variant="filled" fullWidth disabled={!name.trim() || name.trim() === trusted.name} onPress={() => {
                void run(async () => {
                  await store.renameDevice(trusted.id, trusted.revision, name.trim());
                  setSelectedDeviceID(null);
                });
              }} />
            </AppHost>
            {trusted.bindings?.map((binding) => (
              <AppHost key={binding.id} matchContents={{ vertical: true }}>
                <AppButton
                  title={pair(binding.sync_enabled ? "pauseSync" : "resumeSync")}
                  variant="tonal"
                  fullWidth
                  onPress={() => void run(() => store.setBindingSync(binding.id, binding.revision, !binding.sync_enabled))}
                />
              </AppHost>
            ))}
            <AppHost matchContents={{ vertical: true }}>
              <AppButton title={pair("unpair")} variant="outlined" fullWidth onPress={() => {
                setSelectedDeviceID(null);
                void run(() => store.unpairDevice(trusted.id, trusted.revision));
              }} />
            </AppHost>
            <AppHost matchContents={{ vertical: true }}>
              <AppButton title={t("common:action.close")} variant="text" fullWidth onPress={() => setSelectedDeviceID(null)} />
            </AppHost>
          </View>
        )}
      </AppBottomSheet>
    </>
  );
}
