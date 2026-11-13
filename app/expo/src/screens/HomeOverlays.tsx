import { CardContextOverlay } from "@/components/CardContextOverlay";
import { ClipboardDetailModal } from "@/components/ClipboardDetailModal";
import { WordPickerOverlay } from "@/components/WordPickerOverlay";
import { DemoSendSheet } from "@/shell/DemoSendSheet";
import type { HomeController } from "./useHomeController";

export function HomeOverlays({ c }: { c: HomeController }) {
  return (
    <>
      {c.wordPickerTarget && (
        <WordPickerOverlay
          {...c.wordPickerTarget}
          onSendTo={c.openSendToText}
          onDismiss={() => c.setWordPickerTarget(null)}
        />
      )}
      <ClipboardDetailModal
        visible={c.detailPageItem != null}
        onDismiss={c.closeDetailPage}
        c={c}
        item={c.detailPageItem}
      />
      <DemoSendSheet visible={c.sendToVisible} onClose={c.closeSendTo} />
      <CardContextOverlay
        item={c.contextItem}
        displayKind={c.contextDisplayKind}
        anchor={c.contextTarget?.anchor ?? null}
        actionGroups={c.actionMenuGroups}
        onDismiss={c.handleContextDismiss}
      />
    </>
  );
}
