"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { STORAGE_KEY } from "./constants";
import { DEFAULT_PROJECT } from "./defaults";
import { coerceLocalized } from "./locale";
import type { Device, ProjectState, Slide } from "./types";

const HISTORY_LIMIT = 50;
// Coalesce rapid edits (typing, slider drags) into a single undo step.
const COALESCE_MS = 500;
// Debounce file/localStorage writes — frequent enough to feel instant, infrequent enough not to thrash disk.
const SAVE_DEBOUNCE_MS = 600;

// Migrate pre-locale string label/headline into { en: value }.
function migrateSlide(slide: Slide): Slide {
  return {
    ...slide,
    label: coerceLocalized(slide.label as unknown),
    headline: coerceLocalized(slide.headline as unknown),
  };
}

function mergeWithDefaults(parsed: Partial<ProjectState>): ProjectState {
  const slidesByDevice = parsed.slidesByDevice
    ? Object.fromEntries(
        Object.entries(parsed.slidesByDevice).map(([device, slides]) => [
          device,
          (slides || []).map(migrateSlide),
        ]),
      )
    : {};
  const merged: ProjectState = {
    ...DEFAULT_PROJECT,
    ...parsed,
    slidesByDevice: {
      ...DEFAULT_PROJECT.slidesByDevice,
      ...slidesByDevice,
    } as ProjectState["slidesByDevice"],
  };
  // Clamp the active locale into the project's locale list so a stale
  // `locale` (e.g. from a project that dropped languages) doesn't show blank.
  if (!merged.locales || merged.locales.length === 0) {
    merged.locales = [...DEFAULT_PROJECT.locales];
  }
  if (!merged.locales.includes(merged.locale)) {
    merged.locale = merged.locales[0];
  }
  return merged;
}

function loadFromLocalStorage(): ProjectState | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return mergeWithDefaults(JSON.parse(raw) as Partial<ProjectState>);
  } catch {
    return null;
  }
}

async function loadFromFile(): Promise<ProjectState | null> {
  if (typeof window === "undefined") return null;
  try {
    const resp = await fetch("/api/project", { cache: "no-store" });
    if (!resp.ok) return null;
    const json = (await resp.json()) as { ok: boolean; state: Partial<ProjectState> | null };
    if (!json.ok || !json.state) return null;
    return mergeWithDefaults(json.state);
  } catch {
    return null;
  }
}

function saveToLocalStorage(state: ProjectState): { ok: true } | { ok: false; error: string } {
  if (typeof window === "undefined") return { ok: true };
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    return { ok: true };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return { ok: false, error: msg };
  }
}

async function saveToFile(state: ProjectState): Promise<{ ok: true } | { ok: false; error: string }> {
  if (typeof window === "undefined") return { ok: true };
  try {
    const resp = await fetch("/api/project", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(state),
    });
    if (!resp.ok) {
      return { ok: false, error: `HTTP ${resp.status}` };
    }
    const json = (await resp.json()) as { ok: boolean; error?: string };
    if (!json.ok) return { ok: false, error: json.error || "Unknown error" };
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

type Updater = ProjectState | ((prev: ProjectState) => ProjectState);

function applyUpdater(updater: Updater, prev: ProjectState): ProjectState {
  return typeof updater === "function" ? updater(prev) : updater;
}

export function useProject() {
  const [state, _setState] = useState<ProjectState>(DEFAULT_PROJECT);
  const [hydrated, setHydrated] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // History stacks live in refs — they don't drive any rendered UI, so
  // mutating them never needs to re-render.
  const pastRef = useRef<ProjectState[]>([]);
  const futureRef = useRef<ProjectState[]>([]);
  const lastPushAt = useRef(0);

  // Hydrate: prefer file (git-tracked) → localStorage (cache) → defaults.
  // localStorage is consulted first for instant paint, then file overwrites if present.
  useEffect(() => {
    let cancelled = false;
    const cached = loadFromLocalStorage();
    if (cached) _setState(cached);

    void (async () => {
      const fromFile = await loadFromFile();
      if (cancelled) return;
      if (fromFile) {
        _setState(fromFile);
      }
      pastRef.current = [];
      futureRef.current = [];
      lastPushAt.current = 0;
      setHydrated(true);
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // Debounced autosave to BOTH localStorage (fast, offline) and file (git-trackable).
  useEffect(() => {
    if (!hydrated) return;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const localResult = saveToLocalStorage(state);
      void saveToFile(state).then((fileResult) => {
        if (fileResult.ok && localResult.ok) {
          setSavedAt(Date.now());
          setSaveError(null);
        } else if (!fileResult.ok && !localResult.ok) {
          setSaveError(fileResult.error);
        } else if (!fileResult.ok) {
          // Local cache succeeded but file save failed — work isn't git-portable yet.
          setSavedAt(Date.now());
          setSaveError(`File save failed: ${fileResult.error}`);
        } else {
          setSavedAt(Date.now());
          setSaveError(localResult.ok ? null : localResult.error);
        }
      });
    }, SAVE_DEBOUNCE_MS);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [state, hydrated]);

  const setState = useCallback((updater: Updater) => {
    _setState((prev) => {
      const next = applyUpdater(updater, prev);
      if (next === prev) return prev;
      const now = Date.now();
      if (now - lastPushAt.current > COALESCE_MS) {
        pastRef.current.push(prev);
        if (pastRef.current.length > HISTORY_LIMIT) pastRef.current.shift();
        futureRef.current.length = 0;
      }
      lastPushAt.current = now;
      return next;
    });
  }, []);

  const undo = useCallback(() => {
    _setState((cur) => {
      const prev = pastRef.current.pop();
      if (prev === undefined) return cur;
      futureRef.current.push(cur);
      // Reset coalescing so the next edit after an undo creates a fresh history entry.
      lastPushAt.current = 0;
      return prev;
    });
  }, []);

  const redo = useCallback(() => {
    _setState((cur) => {
      const next = futureRef.current.pop();
      if (next === undefined) return cur;
      pastRef.current.push(cur);
      lastPushAt.current = 0;
      return next;
    });
  }, []);

  const reset = useCallback(() => {
    setState(DEFAULT_PROJECT);
  }, [setState]);

  const resetDevice = useCallback((device: Device) => {
    setState((prev) => ({
      ...prev,
      slidesByDevice: {
        ...prev.slidesByDevice,
        [device]: DEFAULT_PROJECT.slidesByDevice[device],
      },
    }));
  }, [setState]);

  return {
    state,
    setState,
    hydrated,
    savedAt,
    saveError,
    reset,
    resetDevice,
    undo,
    redo,
  };
}
