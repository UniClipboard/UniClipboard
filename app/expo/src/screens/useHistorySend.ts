import { useEffect, useRef, useState } from "react";
import { Alert } from "react-native";
import { useTranslation } from "react-i18next";
import { parseEngineError, unicEngineNative } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import type { EngineOperation } from "@/engine/types";

export type HistorySendPhase =
  | "idle"
  | "preparing"
  | "queued"
  | "sending"
  | "confirming"
  | "relay_stored"
  | "success"
  | "failed"
  | "unconfirmed";

export function useHistorySend(visible: boolean, historyID?: string) {
  const { t } = useTranslation();
  const pending = useRef(false);
  const session = useRef(0);
  const [submitting, setSubmitting] = useState(false);
  const [operationID, setOperationID] = useState<string>();
  const [operation, setOperation] = useState<EngineOperation>();
  const [unconfirmed, setUnconfirmed] = useState(false);

  useEffect(() => {
    if (visible) return;
    session.current++;
    pending.current = false;
    setSubmitting(false);
    setOperationID(undefined);
    setOperation(undefined);
    setUnconfirmed(false);
  }, [visible]);

  useEffect(() => {
    if (!visible || !operationID) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      try {
        const next = await unicEngineNative.getOperation(operationID);
        if (cancelled) return;
        setOperation(next);
        if (next.phase !== "completed" && next.phase !== "failed") {
          timer = setTimeout(() => void refresh(), 500);
        }
      } catch {
        if (!cancelled) setUnconfirmed(true);
      }
    };
    void refresh();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [visible, operationID]);

  const send = async (deviceIds: string[]) => {
    if (!historyID || deviceIds.length === 0 || pending.current || operationID) return;
    pending.current = true;
    const current = session.current;
    setSubmitting(true);
    try {
      const result = await useEngineStore.getState().resendHistoryItem(historyID, deviceIds);
      if (session.current !== current) return;
      if (result.sync_disposition === "queued" && result.operation_id) {
        setOperationID(result.operation_id);
      } else {
        const detail = result.sync_disposition === "no_targets"
          ? t("share:send.offline")
          : result.sync_disposition === "disabled" ? t("settings:live.status.sync_disabled") : undefined;
        Alert.alert(t("share:send.notStarted"), detail);
      }
    } catch (error) {
      if (session.current === current) {
        Alert.alert(t("share:send.failed"), t(`errors:engine.${parseEngineError(error).code}`, {
          defaultValue: t("errors:engine.internal"),
        }));
      }
    } finally {
      if (session.current === current) {
        pending.current = false;
        setSubmitting(false);
      }
    }
  };

  const done = unconfirmed || operation?.phase === "completed" || operation?.phase === "failed";
  const busy = submitting || (Boolean(operationID) && !done);
  const queued = Boolean(operationID) && (!operation || operation.phase === "queued");
  const phaseFor = (id: string): HistorySendPhase => {
    if (unconfirmed) return "unconfirmed";
    const targets = operation?.targets?.filter(
      (target) => target.peer_id === id || target.target_id === id,
    ) ?? [];
    const sentTargets = targets.filter((target) => target.state === "sent");
    if (sentTargets.length > 0) {
      if (sentTargets.every((target) => target.receipt_stage === "relay_stored")) {
        return "relay_stored";
      }
      return "success";
    }
    const sendingTargets = targets.filter((target) => target.state === "sending");
    if (sendingTargets.length > 0) {
      if (sendingTargets.some((target) => target.receipt_stage === "local_written")) {
        return "confirming";
      }
      return "sending";
    }
    if (targets.some((target) => target.state === "queued")) return "queued";
    if (done) return "failed";
    if (operationID) return "queued";
    if (submitting) return "preparing";
    return "idle";
  };
  const waitingLabel = queued ? t("share:send.targetState.queued") : t("share:send.sending");
  return { send, busy, done, phaseFor, waitingLabel };
}
