import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Alert, BackHandler, Keyboard } from "react-native";
import { useFocusEffect } from "@react-navigation/native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useTranslation } from "react-i18next";
import { openWebLink, shareHistoryItems } from "@/engine/historyActions";
import { unicEngineNative } from "@/engine/nativeModule";
import { useMessageStore } from "@/stores/messageStore";
import { useEngineStore } from "@/engine/store";
import { useTheme } from "@/hooks/useTheme";
import { useHistoryDisplaySettings } from "@/hooks/useHistoryDisplaySettings";
import type { AnimatedCardGridHandle } from "@/components/AnimatedCardGrid";
import type { CardAnchorRect } from "@/components/CardContextOverlay.types";
import type { WordPickerOverlayProps } from "@/components/WordPickerOverlay.types";
import type { ClipboardItem } from "@/types/clipboard";
import { buildActionMenuGroups } from "@/utils/actionMenuItems";
import { getDisplayKind, type DisplayKind } from "@/utils/displayKind";
import {
  createHistorySearchFilter,
  matchesHistoryFilter,
  type HistoryDateFilter,
  type HistorySourceFilter,
} from "@/utils/historyFilters";
import { useShellStore } from "@/shell/store";
import { showUnavailable } from "@/shell/notice";
import { useSendSheetStore } from "./sendSheetStore";

function filterLiveItems(
  records: ClipboardItem[],
  selectedFilterKinds: DisplayKind[],
): ClipboardItem[] {
  // Engine already owns keyword search (over full text beyond 512B), date range,
  // source filtering, and coarse type filtering. Only narrow client-side when
  // the user explicitly selected the URL sub-kind (which Engine stores as text).
  if (selectedFilterKinds.includes("url") && !selectedFilterKinds.includes("text")) {
    return records.filter((item) => getDisplayKind(item.type, item.text) === "url");
  }
  return records;
}

export function useHomeController(
  onOpenSettings: () => void,
  { clearFiltersOnCloseSearch }: { clearFiltersOnCloseSearch: boolean },
) {
  const { t } = useTranslation("home");
  const { theme } = useTheme();
  const insets = useSafeAreaInsets();
  const engineMode = useEngineStore((s) => s.mode);
  const engineRecords = useEngineStore((s) => s.records);
  const availableTags = useEngineStore((s) => s.availableTags);
  const engineActions = useEngineStore((s) => s.actions);
  const engineReceived = useEngineStore((s) => s.received);
  const shellRecords = useShellStore((s) => s.records);
  const records = engineMode === "live" ? engineRecords : shellRecords;
  const [selectedIds, setSelectedIds] = useState(new Set<string>());
  const [isSelectMode, setIsSelectMode] = useState(false);
  const [isSearching, setIsSearching] = useState(false);
  const [selectedTag, setSelectedTag] = useState<string | null>(null);
  const [tagEditID, setTagEditID] = useState<string | null>(null);
  const [searchText, setSearchText] = useState("");
  const [selectedFilterKinds, setSelectedFilterKinds] = useState<DisplayKind[]>(
    [],
  );
  const [selectedDateFilter, setSelectedDateFilter] =
    useState<HistoryDateFilter>("all");
  const [selectedSourceFilter, setSelectedSourceFilter] =
    useState<HistorySourceFilter>("all");
  const [showAddMenu, setShowAddMenu] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [contextTarget, setContextTarget] = useState<{
    item: ClipboardItem;
    anchor: CardAnchorRect | null;
  } | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const [detailPageId, setDetailPageId] = useState<string | null>(null);
  const [wordPickerTarget, setWordPickerTarget] =
    useState<WordPickerOverlayProps | null>(null);
  const [receiveResultsOpen, setReceiveResultsOpen] = useState(false);
  const listRef = useRef<AnimatedCardGridHandle>(null);
  const {
    historyLayout,
    setHistoryLayout,
    isLoading: isHistoryLayoutLoading,
  } = useHistoryDisplaySettings();
  const filter = useMemo(
    () =>
      createHistorySearchFilter({
        keyword: isSearching ? searchText : "",
        displayKinds: selectedFilterKinds,
        dateFilter: selectedDateFilter,
        sourceFilter: selectedSourceFilter,
      }),
    [
      isSearching,
      searchText,
      selectedFilterKinds,
      selectedDateFilter,
      selectedSourceFilter,
    ],
  );
  const items = useMemo(
    () =>
      engineMode === "live"
        ? filterLiveItems(records, selectedFilterKinds)
        : records.filter((item) => matchesHistoryFilter(item, filter)),
    [engineMode, records, selectedFilterKinds, filter],
  );
  const detailItem =
    records.find((item) => item.profileHash === detailId) ?? null;
  const detailPageItem =
    records.find((item) => item.profileHash === detailPageId) ?? null;
  const contextItem =
    records.find(
      (item) => item.profileHash === contextTarget?.item.profileHash,
    ) ?? null;
  const contextDisplayKind = contextItem
    ? getDisplayKind(contextItem.type, contextItem.text)
    : null;
  const clearSelection = useCallback(
    () => setSelectedIds(new Set<string>()),
    [],
  );
  const toggleSelection = useCallback(
    (id: string) =>
      setSelectedIds((current) => {
        const next = new Set(current);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      }),
    [],
  );
  const exitSelectMode = useCallback(() => {
    setIsSelectMode(false);
    clearSelection();
  }, [clearSelection]);
  const handleClearFilters = useCallback(() => {
    setSelectedFilterKinds([]);
    setSelectedTag(null);
    setSelectedDateFilter("all");
    setSelectedSourceFilter("all");
  }, []);
  const closeSearch = useCallback(() => {
    setIsSearching(false);
    setSearchText("");
    if (clearFiltersOnCloseSearch) handleClearFilters();
  }, [clearFiltersOnCloseSearch, handleClearFilters]);
  const openSearch = useCallback(() => {
    setShowAddMenu(false);
    setIsSearching(true);
  }, []);
  const openDetailPage = useCallback((item: ClipboardItem) => {
    Keyboard.dismiss();
    setDetailPageId(item.profileHash);
  }, []);
  const closeDetailPage = useCallback(() => setDetailPageId(null), []);
  const openSendTo = useCallback((item: ClipboardItem) => {
    useSendSheetStore.getState().open({ item });
  }, []);
  const openSendToText = useCallback((draftText: string) => {
    useSendSheetStore.getState().open({ draftText });
  }, []);
  const handleContextDismiss = useCallback(() => setContextTarget(null), []);
  const handleItemDelete = useCallback(
    (item: ClipboardItem) => {
      if (engineMode === "live") {
        void useEngineStore.getState().deleteHistoryItems([item.profileHash]);
      } else {
        useShellStore.getState().deleteRecords([item.profileHash]);
      }
    },
    [engineMode],
  );
  const handleItemCopy = useCallback(
    async (item: ClipboardItem) => {
      if (engineMode !== "live") {
        showUnavailable();
        return false;
      }
      try {
        await useEngineStore.getState().applyToClipboard(item.profileHash);
        return true;
      } catch (err) {
        const msg = err instanceof Error ? err.message : t("shell:unavailable");
        Alert.alert(t("common:action.copy"), msg);
        return false;
      }
    },
    [engineMode, t],
  );
  const handleItemPress = useCallback(
    (item: ClipboardItem) => {
      if (isSelectMode) toggleSelection(item.profileHash);
      else openDetailPage(item);
    },
    [isSelectMode, toggleSelection, openDetailPage],
  );
  const handleItemLongPress = useCallback(
    (item: ClipboardItem, anchor: CardAnchorRect | null) =>
      setContextTarget({ item, anchor }),
    [],
  );
  const runHistoryAction = useCallback(
    (action: () => Promise<unknown>): Promise<void> => {
      if (engineMode !== "live") {
        showUnavailable();
        return Promise.resolve();
      }
      return action().then(
        () => undefined,
        (error: unknown) => {
          Alert.alert(
            t("history:media.actionFailed"),
            error instanceof Error ? error.message : undefined,
          );
        },
      );
    },
    [engineMode, t],
  );
  const saveHistoryMedia = useCallback((item: ClipboardItem, destination: "photos" | "files") => {
    runHistoryAction(async () => {
      const saved = await unicEngineNative.exportHistoryMedia(item.profileHash, destination);
      if (saved) useMessageStore.getState().showMessage(t("history:media.saved"), "success");
    });
  }, [runHistoryAction, t]);

  const makeActionGroups = useCallback(
    (item: ClipboardItem, kind: DisplayKind, anchor: CardAnchorRect | null) => {
      const baseGroups = buildActionMenuGroups(item, kind, {
        onCopy: () => void handleItemCopy(item),
        onCopyPlainText: () => runHistoryAction(async () => {
          const text = await unicEngineNative.readHistoryFullText(item.profileHash);
          if (text.truncated) throw new Error(t("history:media.textTooLong"));
          await unicEngineNative.copyPlainText(text.text);
          useMessageStore.getState().showMessage(t("history:wordPicker.copied"), "success");
        }),
        onSelectText: () => {
          if (engineMode === "live" && item.type === "Text") {
            void useEngineStore
              .getState()
              .readHistoryFullText(item.profileHash)
              .then((fullText) => {
                setWordPickerTarget({
                  text: fullText || item.text,
                  anchor,
                  deviceName: item.deviceName,
                  onDismiss: () => setWordPickerTarget(null),
                });
              })
              .catch(() => {
                setWordPickerTarget({
                  text: item.text,
                  anchor,
                  deviceName: item.deviceName,
                  onDismiss: () => setWordPickerTarget(null),
                });
              });
          } else {
            setWordPickerTarget({
              text: item.text,
              anchor,
              deviceName: item.deviceName,
              onDismiss: () => setWordPickerTarget(null),
            });
          }
        },
        onOpenInBrowser: () => runHistoryAction(async () => {
          const text = await unicEngineNative.readHistoryFullText(item.profileHash);
          if (text.truncated) throw new Error(t("history:media.textTooLong"));
          await openWebLink(text.text);
        }),
        onSaveImage: () => saveHistoryMedia(item, "photos"),
        onSaveFile: () => saveHistoryMedia(item, "files"),
        onResend: () => openSendTo(item),
        onShare: () => runHistoryAction(() => shareHistoryItems([item])),
        onSelect: () => {
          setIsSelectMode(true);
          setSelectedIds(new Set([item.profileHash]));
        },
        onDelete: () => handleItemDelete(item),
      });
      if (engineMode === "live") {
        baseGroups.push([{
          key: "togglePin",
          label: t(item.pinned ? "history:tags.unpin" : "history:tags.pin"),
          icon: "pin-outline",
          onPress: () => runHistoryAction(() => useEngineStore.getState().togglePin(item.profileHash)),
        }, {
          key: "editTags",
          label: t("history:tags.edit"),
          icon: "pricetag-outline",
          onPress: () => setTagEditID(item.profileHash),
        }]);
        baseGroups.push([{
          key: "sendTo",
          label: t("detail.sendTo"),
          icon: "paper-plane-outline",
          onPress: () => openSendTo(item),
        }]);
      }
      if (engineMode === "live" && (kind === "text" || kind === "url")) {
        const textActions = engineActions
          .filter((act) => act.accepts?.includes("text"))
          .map((act) => ({
            key: `engine-action:${act.key}`,
            label: act.title,
            icon: "sparkles-outline",
            onPress: () => {
              void useEngineStore
                .getState()
                .runAction(act.key, item.profileHash)
                .catch((err) => {
                  const msg = err instanceof Error ? err.message : t("shell:unavailable");
                  Alert.alert(act.title, msg);
                });
            },
          }));
        if (textActions.length > 0 && baseGroups.length > 0) {
          return [baseGroups[0], textActions, ...baseGroups.slice(1)];
        }
      }
      return baseGroups;
    },
    [engineMode, engineActions, handleItemCopy, handleItemDelete, runHistoryAction, saveHistoryMedia, openSendTo, t],
  );
  const actionMenuGroups = useMemo(
    () =>
      contextItem && contextDisplayKind
        ? makeActionGroups(
            contextItem,
            contextDisplayKind,
            contextTarget?.anchor ?? null,
          )
        : [],
    [contextItem, contextDisplayKind, contextTarget, makeActionGroups],
  );
  const selectionItemActions = useMemo(() => {
    const item = items.find((item) => selectedIds.has(item.profileHash));
    return selectedIds.size === 1 && item
      ? makeActionGroups(
          item,
          getDisplayKind(item.type, item.text),
          null,
        ).flat()
      : [];
  }, [items, selectedIds, makeActionGroups]);
  useEffect(() => {
    setSelectedIds(
      (current) =>
        new Set(
          [...current].filter((id) =>
            items.some((item) => item.profileHash === id),
          ),
        ),
    );
  }, [items]);
  useFocusEffect(
    useCallback(() => {
      const subscription = BackHandler.addEventListener(
        "hardwareBackPress",
        () => {
          if (detailPageItem) {
            closeDetailPage();
            return true;
          }
          if (isSelectMode) {
            exitSelectMode();
            return true;
          }
          if (isSearching) {
            closeSearch();
            return true;
          }
          return false;
        },
      );
      return () => subscription.remove();
    }, [
      detailPageItem,
      closeDetailPage,
      isSelectMode,
      exitSelectMode,
      isSearching,
      closeSearch,
    ]),
  );
  useEffect(() => {
    if (engineMode !== "live") return;
    void useEngineStore.getState().setQueryFilters({
      text: isSearching ? searchText : "",
      tags: selectedTag ? [selectedTag] : [],
      kinds: selectedFilterKinds,
      dateFilter: selectedDateFilter,
      sourceFilter: selectedSourceFilter,
    });
  }, [
    engineMode,
    isSearching,
    searchText,
    selectedTag,
    selectedFilterKinds,
    selectedDateFilter,
    selectedSourceFilter,
  ]);

  const emptyContent = {
    icon: "search-outline" as const,
    tint: theme.colors.textSecondary,
    title: engineMode === "live" ? t("home:empty.online.title") : t("shell:empty"),
    description: engineMode === "live" ? t("home:empty.online.description") : t("shell:localOnly"),
  };
  return {
    t,
    theme,
    insets,
    onOpenSettings,
    items,
    baseItems: records,
    availableTags,
    selectedTag,
    setSelectedTag,
    tagEditItem: records.find((item) => item.profileHash === tagEditID) ?? null,
    closeTagEditor: () => setTagEditID(null),
    filter,
    resultCount: items.length,
    isHistoryLoading: false,
    isInitialHistoryLoadComplete: true,
    latestId: records[0]?.profileHash,
    historyLayout,
    setHistoryLayout,
    isHistoryLayoutLoading,
    emptyContent,
    selectedIds,
    allSelected:
      items.length > 0 &&
      items.every((item) => selectedIds.has(item.profileHash)),
    isSelectMode,
    setIsSelectMode,
    clearSelection,
    toggleSelection,
    exitSelectMode,
    handleSelectAll: () =>
      setSelectedIds(
        selectedIds.size === items.length
          ? new Set()
          : new Set(items.map((item) => item.profileHash)),
      ),
    selectionItemActions,
    isSearching,
    openSearch,
    closeSearch,
    searchText,
    setSearchText,
    resetSearch: () => {
      setSearchText("");
      handleClearFilters();
    },
    selectedFilterKinds,
    selectedDateFilter,
    setSelectedDateFilter,
    selectedSourceFilter,
    setSelectedSourceFilter,
    hasActiveFilters:
      selectedFilterKinds.length > 0 ||
      selectedTag !== null ||
      selectedDateFilter !== "all" ||
      selectedSourceFilter !== "all",
    handleSelectFilterKind: (kind: DisplayKind | null) =>
      setSelectedFilterKinds(kind ? [kind] : []),
    handleToggleFilterKind: (kind: DisplayKind) =>
      setSelectedFilterKinds((current) =>
        current.includes(kind) ? [] : [kind],
      ),
    handleClearFilters,
    handleClearFilterKinds: () => setSelectedFilterKinds([]),
    listRef,
    keyExtractor: (item: ClipboardItem) => item.profileHash,
    handleItemPress,
    handleItemDoublePress: handleItemCopy,
    handleItemCopy,
    handleItemDelete,
    handleItemLongPress,
    refreshing,
    handleRefresh: () => {
      if (engineMode !== "live") return;
      setRefreshing(true);
      void useEngineStore
        .getState()
        .refreshAll()
        .finally(() => setRefreshing(false));
    },
    loadMoreItems: () => {
      if (engineMode === "live") {
        void useEngineStore.getState().loadMoreItems();
      }
    },
    reportVisibleHistoryItems: (_items: ClipboardItem[]) => {},
    handleBatchCopy: () => {
      const first = items.find((it) => selectedIds.has(it.profileHash));
      if (!first) return;
      void handleItemCopy(first);
      exitSelectMode();
    },
    handleBatchShare: () => {
      const selected = items.filter((item) => selectedIds.has(item.profileHash));
      if (selected.length > 0) runHistoryAction(() => shareHistoryItems(selected));
    },
    handleBatchSendTo: () => {
      const first = items.find((it) => selectedIds.has(it.profileHash));
      if (first) openSendTo(first);
    },
    handleBatchDelete: () => {
      if (engineMode === "live") {
        void useEngineStore.getState().deleteHistoryItems([...selectedIds]);
      } else {
        useShellStore.getState().deleteRecords([...selectedIds]);
      }
      exitSelectMode();
    },
    showAddMenu,
    setShowAddMenu,
    handleTakePhoto: () => {
      setShowAddMenu(false);
      runHistoryAction(() => unicEngineNative.pickAndCommitImage(true));
    },
    handleUploadImage: () => {
      setShowAddMenu(false);
      runHistoryAction(() => unicEngineNative.pickAndCommitImage(false));
    },
    handleUploadFile: () => {
      setShowAddMenu(false);
      runHistoryAction(() => useEngineStore.getState().pickAndCommitFile());
    },
    handleUpload: () => {
      setShowAddMenu(false);
      runHistoryAction(() => useEngineStore.getState().readSystemClipboardAndCommit());
    },
    handleSyncHistory: () =>
      runHistoryAction(() => useEngineStore.getState().readSystemClipboardAndCommit()),
    receiveEntryCount: engineMode === "live" ? engineReceived.length : 0,
    receiveResultsOpen,
    setReceiveResultsOpen: (open: boolean) => {
      if (engineMode !== "live") {
        showUnavailable();
        return;
      }
      if (open) {
        void useEngineStore.getState().refreshReceived();
      }
      setReceiveResultsOpen(open);
    },
    wordPickerTarget,
    setWordPickerTarget,
    contextItem,
    contextTarget,
    contextDisplayKind,
    actionMenuGroups,
    handleContextDismiss,
    detailItem,
    selectDetailItem: (item: ClipboardItem) => setDetailId(item.profileHash),
    makeActionGroups,
    detailPageItem,
    openDetailPage,
    closeDetailPage,
    openSendToText,
  };
}
export type HomeController = ReturnType<typeof useHomeController>;
