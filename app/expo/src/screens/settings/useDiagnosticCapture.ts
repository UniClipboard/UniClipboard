import { useCallback, useEffect, useRef, useState } from "react";
import { type DiagnosticLogLevelState, unicEngineNative } from "@/engine/nativeModule";

export const DETAILED_CAPTURE_DURATION_SECONDS = 600;

export type DiagnosticWarningKey = "engineLogsMissing" | "engineLogsUnreadable";

export function mapDiagnosticWarningKeys(warnings: readonly string[] | undefined): DiagnosticWarningKey[] {
  if (!warnings?.length) return [];
  const keys: DiagnosticWarningKey[] = [];
  for (const warning of warnings) {
    if (warning === "engine_logs_missing" && !keys.includes("engineLogsMissing")) {
      keys.push("engineLogsMissing");
    } else if (warning === "engine_logs_unreadable" && !keys.includes("engineLogsUnreadable")) {
      keys.push("engineLogsUnreadable");
    }
  }
  return keys;
}

function activeOverrideExpiresAt(state: DiagnosticLogLevelState | null): number | null {
  if (!state) return null;
  const local = state.local;
  if (local.source !== "override" || local.effective_level !== "debug" || local.expires_at_ms === null) {
    return null;
  }
  return local.expires_at_ms > Date.now() ? local.expires_at_ms : null;
}

export type DiagnosticCaptureState = {
  available: boolean;
  active: boolean;
  busy: boolean;
  failed: boolean;
  remainingMinutes: number;
  toggle: () => Promise<void>;
};

export function useDiagnosticCapture(): DiagnosticCaptureState {
  const [available, setAvailable] = useState(false);
  const [expiresAtMs, setExpiresAtMs] = useState<number | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const busyRef = useRef(false);

  const applyState = useCallback((state: DiagnosticLogLevelState) => {
    setAvailable(true);
    setExpiresAtMs(activeOverrideExpiresAt(state));
    setNowMs(Date.now());
  }, []);

  const refresh = useCallback(async () => {
    try {
      const state = await unicEngineNative.getDiagnosticLogLevelState();
      applyState(state);
    } catch {
      setAvailable(false);
      setExpiresAtMs(null);
    }
  }, [applyState]);

  useEffect(() => {
    let live = true;
    void unicEngineNative.getDiagnosticLogLevelState().then(
      (state) => {
        if (live) applyState(state);
      },
      () => {
        if (live) {
          setAvailable(false);
          setExpiresAtMs(null);
        }
      },
    );
    return () => {
      live = false;
    };
  }, [applyState]);

  useEffect(() => {
    if (expiresAtMs === null) return;
    const timer = setInterval(() => {
      const current = Date.now();
      setNowMs(current);
      if (current >= expiresAtMs) {
        void refresh();
      }
    }, 15_000);
    return () => clearInterval(timer);
  }, [expiresAtMs, refresh]);

  const active = expiresAtMs !== null && expiresAtMs > nowMs;
  const remainingMinutes = active ? Math.max(1, Math.ceil((expiresAtMs - nowMs) / 60_000)) : 0;

  const toggle = useCallback(async () => {
    if (busyRef.current || !available) return;
    busyRef.current = true;
    setBusy(true);
    setFailed(false);
    try {
      const next = active
        ? await unicEngineNative.setLocalLogLevelOverride("", 0, true)
        : await unicEngineNative.setLocalLogLevelOverride("debug", DETAILED_CAPTURE_DURATION_SECONDS, false);
      applyState(next);
    } catch {
      setFailed(true);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [active, applyState, available]);

  return {
    available,
    active,
    busy,
    failed,
    remainingMinutes,
    toggle,
  };
}
