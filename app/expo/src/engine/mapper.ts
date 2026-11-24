import { type ClipboardContentType } from "@/types/api";
import { type ClipboardItem, HistorySyncStatus } from "@/types/clipboard";
import type { EngineDevice, HistoryItem, ImagePreviewResult, TextPreview } from "./types";

function mapKindToContentType(item: HistoryItem): ClipboardContentType {
  switch (item.kind) {
    case "image":
      return "Image";
    case "files":
    case "file":
      return item.parts.filter((p) => p.kind === "file").length > 1 ? "Group" : "File";
    case "directory":
    case "directory_reference":
      return "Group";
    case "mixed":
      return item.parts.some((p) => p.kind === "file") ? "Group" : "Text";
    case "text":
    case "html":
    default:
      return "Text";
  }
}

export function mapEngineHistoryItem(
  item: HistoryItem,
  deviceNames: ReadonlyMap<string, string>,
  preview?: TextPreview,
  imagePreview?: ImagePreviewResult,
  fallbackSearchText?: string,
): ClipboardItem {
  const type = mapKindToContentType(item);
  const fileParts = item.parts.filter((p) => p.kind === "file");
  const primaryName =
    imagePreview?.name ||
    (type === "Image"
      ? "Image"
      : fileParts.length > 1
        ? `${fileParts.length} files`
        : fileParts.length === 1
          ? "File"
          : undefined);

  const displayText =
    preview?.text && preview.text.length > 0
      ? preview.text
      : fallbackSearchText && fallbackSearchText.length > 0
        ? fallbackSearchText
        : primaryName ?? (type === "Text" ? "" : `[${type}]`);

  const tags = item.tags ?? [];
  const originDeviceName =
    item.origin.kind === "remote_device" && item.origin.device_id
      ? deviceNames.get(item.origin.device_id) ?? item.origin.device_id.slice(0, 8)
      : item.origin.kind === "migration"
        ? "v1"
        : item.origin.kind === "action"
          ? "Action"
          : item.source_application?.name;

  return {
    type,
    text: displayText,
    profileHash: item.id,
    hasData: type !== "Text" || Boolean(preview?.html),
    dataName: primaryName,
    size: item.total_bytes || undefined,
    timestamp: item.created_at_ms,
    lastModified: item.updated_at_ms,
    lastAccessed: item.updated_at_ms,
    deviceName: originDeviceName,
    version: Number(item.revision) || 1,
    incarnationId: item.revision,
    starred: tags.includes("starred"),
    pinned: tags.includes("pinned"),
    isDeleted: false,
    isLocalFileReady: item.parts.some((p) => p.available),
    syncStatus:
      item.sync_state === "synced"
        ? HistorySyncStatus.Synced
        : item.sync_state === "pending"
          ? HistorySyncStatus.NeedSync
          : HistorySyncStatus.LocalOnly,
    fileUri: imagePreview?.uri,
    from: item.origin.kind === "remote_device" ? "remote" : undefined,
  };
}

export function buildDeviceNameMap(devices: readonly EngineDevice[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const d of devices) {
    map.set(d.id, d.name || d.reported_name || d.id.slice(0, 8));
  }
  return map;
}
