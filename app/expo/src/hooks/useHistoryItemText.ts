import { useEffect, useState } from "react";
import { useEngineStore } from "@/engine/store";
import type { ClipboardItem } from "@/types/clipboard";

/**
 * Loads full UTF-8 text on demand for detail views in live Engine mode
 * while keeping list rows backed by lightweight 512 B previews.
 * Clears its in-memory reference on unmount or when switching items.
 */
export function useHistoryItemText(item: ClipboardItem): string {
  const mode = useEngineStore((s) => s.mode);
  const [fullText, setFullText] = useState<string | null>(null);

  useEffect(() => {
    setFullText(null);
    if (mode !== "live" || item.type !== "Text" || !item.profileHash) {
      return;
    }
    let cancelled = false;
    void useEngineStore
      .getState()
      .readHistoryFullText(item.profileHash)
      .then((loaded) => {
        if (!cancelled && loaded && loaded.length > 0) {
          setFullText(loaded);
        }
      })
      .catch(() => {
        // Keep preview text on transient read failure.
      });
    return () => {
      cancelled = true;
      setFullText(null);
    };
  }, [mode, item.profileHash, item.incarnationId, item.type]);

  return fullText ?? item.text;
}
