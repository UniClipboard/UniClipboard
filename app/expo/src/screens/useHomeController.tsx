import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BackHandler } from "react-native";
import { useFocusEffect } from "@react-navigation/native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { useTranslation } from "react-i18next";
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

export function useHomeController(
  onOpenSettings: () => void,
  { clearFiltersOnCloseSearch }: { clearFiltersOnCloseSearch: boolean },
) {
  const { t } = useTranslation("home");
  const { theme } = useTheme();
  const insets = useSafeAreaInsets();
  const records = useShellStore((s) => s.records);
  const [selectedIds, setSelectedIds] = useState(new Set<string>());
  const [isSelectMode, setIsSelectMode] = useState(false);
  const [isSearching, setIsSearching] = useState(false);
  const [searchText, setSearchText] = useState("");
  const [selectedFilterKinds, setSelectedFilterKinds] = useState<DisplayKind[]>(
    [],
  );
  const [selectedDateFilter, setSelectedDateFilter] =
    useState<HistoryDateFilter>("all");
  const [selectedSourceFilter, setSelectedSourceFilter] =
    useState<HistorySourceFilter>("all");
  const [showAddMenu, setShowAddMenu] = useState(false);
  const [contextTarget, setContextTarget] = useState<{
    item: ClipboardItem;
    anchor: CardAnchorRect | null;
  } | null>(null);
  const [detailId, setDetailId] = useState<string | null>(null);
  const [detailPageId, setDetailPageId] = useState<string | null>(null);
  const [wordPickerTarget, setWordPickerTarget] =
    useState<WordPickerOverlayProps | null>(null);
  const [sendToVisible, setSendToVisible] = useState(false);
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
    () => records.filter((item) => matchesHistoryFilter(item, filter)),
    [records, filter],
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
  const openDetailPage = useCallback(
    (item: ClipboardItem) => setDetailPageId(item.profileHash),
    [],
  );
  const closeDetailPage = useCallback(() => setDetailPageId(null), []);
  const openSendTo = useCallback(
    (_item?: ClipboardItem) => setSendToVisible(true),
    [],
  );
  const closeSendTo = useCallback(() => setSendToVisible(false), []);
  const handleContextDismiss = useCallback(() => setContextTarget(null), []);
  const handleItemDelete = useCallback(
    (item: ClipboardItem) =>
      useShellStore.getState().deleteRecords([item.profileHash]),
    [],
  );
  const handleItemCopy = useCallback(async (_item: ClipboardItem) => {
    showUnavailable();
    return false;
  }, []);
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
  const makeActionGroups = useCallback(
    (item: ClipboardItem, kind: DisplayKind, anchor: CardAnchorRect | null) =>
      buildActionMenuGroups(item, kind, {
        onCopy: () => void handleItemCopy(item),
        onCopyPlainText: showUnavailable,
        onSelectText: () =>
          setWordPickerTarget({
            text: item.text,
            anchor,
            deviceName: item.deviceName,
            onDismiss: () => setWordPickerTarget(null),
          }),
        onOpenInBrowser: showUnavailable,
        onSaveImage: showUnavailable,
        onSaveFile: showUnavailable,
        onResend: showUnavailable,
        onShare: showUnavailable,
        onSelect: () => {
          setIsSelectMode(true);
          setSelectedIds(new Set([item.profileHash]));
        },
        onDelete: () => handleItemDelete(item),
      }),
    [handleItemCopy, handleItemDelete],
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
  const emptyContent = {
    icon: "search-outline" as const,
    tint: theme.colors.textSecondary,
    title: t("shell:empty"),
    description: t("shell:localOnly"),
  };
  return {
    t,
    theme,
    insets,
    onOpenSettings,
    items,
    baseItems: records,
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
    refreshing: false,
    handleRefresh: () => {},
    loadMoreItems: () => {},
    reportVisibleHistoryItems: (_items: ClipboardItem[]) => {},
    handleBatchCopy: showUnavailable,
    handleBatchShare: showUnavailable,
    handleBatchSendTo: () => setSendToVisible(true),
    handleBatchDelete: () => {
      useShellStore.getState().deleteRecords([...selectedIds]);
      exitSelectMode();
    },
    showAddMenu,
    setShowAddMenu,
    handleTakePhoto: showUnavailable,
    handleUploadImage: showUnavailable,
    handleUploadFile: showUnavailable,
    handleUpload: showUnavailable,
    handleSyncHistory: showUnavailable,
    receiveEntryCount: 0,
    setReceiveResultsOpen: (_open: boolean) => showUnavailable(),
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
    sendToVisible,
    openSendTo,
    closeSendTo,
    openSendToText: () => setSendToVisible(true),
  };
}
export type HomeController = ReturnType<typeof useHomeController>;
