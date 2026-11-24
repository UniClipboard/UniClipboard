import { CardContextOverlay } from "@/components/CardContextOverlay";
import { ClipboardDetailModal } from "@/components/ClipboardDetailModal";
import { ReceiveResultsSheet } from "@/components/ReceiveResultsSheet";
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
      <DemoSendSheet
        visible={c.sendToVisible}
        onClose={c.closeSendTo}
        targetItem={c.sendTargetItem}
        initialDraftText={c.sendDraftText}
      />
      <ReceiveResultsSheet
        visible={c.receiveResultsOpen}
        onClose={() => c.setReceiveResultsOpen(false)}
      />
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
