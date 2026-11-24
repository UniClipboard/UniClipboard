import { Linking } from "react-native";
import i18n from "@/i18n";
import type { ClipboardItem } from "@/types/clipboard";
import { unicEngineNative } from "./nativeModule";

export async function openWebLink(value: string) {
  const text = value.trim();
  if (!/^https?:\/\//i.test(text) || /\s/.test(text)) {
    throw new Error(i18n.t("history:media.invalidLink"));
  }
  const url = new URL(text);
  if (!url.hostname || url.username || url.password) {
    throw new Error(i18n.t("history:media.invalidLink"));
  }
  await Linking.openURL(url.toString());
}

async function shareableText(item: ClipboardItem) {
  const text = await unicEngineNative.readHistoryFullText(item.profileHash);
  if (text.truncated) throw new Error(i18n.t("history:media.textTooLong"));
  return text.text || text.html || "";
}

export async function shareHistoryItems(items: ClipboardItem[]) {
  const text = await Promise.all(items.filter((item) => item.type === "Text").map(shareableText));
  const files = items.filter((item) => item.type !== "Text").map((item) => item.profileHash);
  await unicEngineNative.shareHistoryMedia(files, text.filter(Boolean).join("\n\n"));
}
