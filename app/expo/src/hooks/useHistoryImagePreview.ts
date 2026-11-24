import { useCallback, useEffect, useRef, useState } from "react";
import { unicEngineNative } from "@/engine/nativeModule";
import { useEngineStore } from "@/engine/store";
import type { ClipboardItem } from "@/types/clipboard";

export function useHistoryImagePreview(item: ClipboardItem | null, loadMissing = false) {
  const id = item?.type === "Image" && item.isLocalFileReady ? item.profileHash : undefined;
  const initial = id ? item?.fileUri : undefined;
  const key = JSON.stringify([id, initial]);
  const active = useRef("");
  const attempted = useRef("");
  const [result, setResult] = useState<{ key: string; uri?: string; retry: number }>();
  const isLive = useEngineStore((state) => state.mode === "live");

  const reload = useCallback(async () => {
    if (!id || !isLive || attempted.current === key) {
      setResult({ key, retry: 0 });
      return;
    }
    attempted.current = key;
    try {
      const preview = await unicEngineNative.exportHistoryImagePreview(id);
      if (active.current === key) setResult({ key, uri: preview.uri, retry: 1 });
    } catch {
      if (active.current === key) setResult({ key, retry: 0 });
    }
  }, [id, isLive, key]);

  useEffect(() => {
    active.current = key;
    attempted.current = "";
    if (loadMissing && id && !initial) void reload();
    return () => { active.current = ""; };
  }, [id, initial, key, loadMissing, reload]);

  const current = result?.key === key ? result : undefined;
  return {
    uri: current ? current.uri : initial,
    retry: current?.retry ?? 0,
    onError: () => void reload(),
  };
}
