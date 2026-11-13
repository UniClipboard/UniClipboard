import i18n from '@/i18n';
import type { ClipboardItem } from '@/types/clipboard';
import type { DisplayKind } from '@/utils/displayKind';

export interface ActionMenuItem {
  key: string;
  label: string;
  icon: string;
  destructive?: boolean;
  onPress: () => void;
}

interface ActionHandlers {
  onCopy: () => void;
  onSelectText: () => void;
  onCopyPlainText: () => void;
  onOpenInBrowser: () => void;
  onSaveImage: () => void;
  onSaveFile: () => void;
  onResend: () => void;
  onShare: () => void;
  onSelect: () => void;
  onDelete: () => void;
}

/**
 * 卡片长按菜单的动作分组，从上到下按使用频率与危险度排布：
 * primary（复制类主操作）→ content（针对内容类型的动作）→ organize（分享/多选）→ danger（删除）。
 * 渲染层按组间插入分隔，组内平铺。
 */
export type CardActionGroup = 'primary' | 'content' | 'organize' | 'danger';

const GROUP_ORDER: CardActionGroup[] = ['primary', 'content', 'organize', 'danger'];

/**
 * 卡片长按菜单的动作描述，平台无关（不含 handler、不含图标——两个平台的图标
 * 分别在各自的渲染层按 key 映射）。Android 与 iOS 共用同一份
 * "哪些动作对哪种 displayKind 可见"的判定，避免两处平台实现各自维护一份条件分支。
 */
export interface CardActionDescriptor {
  key: string;
  label: string;
  androidIcon: string;
  group: CardActionGroup;
  destructive?: boolean;
}

export function getClipboardCardActionDescriptors(
  item: ClipboardItem,
  displayKind: DisplayKind
): CardActionDescriptor[] {
  const descriptors: CardActionDescriptor[] = [];

  descriptors.push({
    key: 'copy',
    label: i18n.t('common:action.copy'),
    androidIcon: 'copy-outline',
    group: 'primary',
  });

  if (displayKind === 'text' || displayKind === 'url') {
    descriptors.push({
      key: 'copyPlain',
      label: i18n.t('history:menu.copyPlain'),
      androidIcon: 'clipboard-outline',
      group: 'primary',
    });
    descriptors.push({
      key: 'selectText',
      label: i18n.t('history:menu.selectText'),
      androidIcon: 'text-outline',
      group: 'content',
    });
  }

  if (displayKind === 'url') {
    descriptors.push({
      key: 'openBrowser',
      label: i18n.t('history:menu.openBrowser'),
      androidIcon: 'open-outline',
      group: 'content',
    });
  }

  if (displayKind === 'image' && item.isLocalFileReady && item.fileUri) {
    descriptors.push({
      key: 'saveImage',
      label: i18n.t('history:menu.saveImage'),
      androidIcon: 'download-outline',
      group: 'content',
    });
  }

  if (
    (displayKind === 'file' || displayKind === 'group') &&
    item.isLocalFileReady &&
    item.fileUri
  ) {
    descriptors.push({
      key: 'saveFile',
      label: i18n.t('history:menu.saveFile'),
      androidIcon: 'download-outline',
      group: 'content',
    });
  }

  if (item.p2pEntryId && item.p2pDeliveryState && item.p2pDeliveryState !== 'delivered') {
    descriptors.push({
      key: 'resend',
      label: i18n.t('history:menu.resend'),
      androidIcon: 'refresh-outline',
      group: 'primary',
    });
  }

  descriptors.push({
    key: 'share',
    label: i18n.t('common:action.share'),
    androidIcon: 'share-outline',
    group: 'organize',
  });
  descriptors.push({
    key: 'select',
    label: i18n.t('common:action.select'),
    androidIcon: 'checkmark-circle-outline',
    group: 'organize',
  });
  descriptors.push({
    key: 'delete',
    label: i18n.t('common:action.delete'),
    androidIcon: 'trash-outline',
    group: 'danger',
    destructive: true,
  });

  return descriptors;
}

export function buildActionMenuGroups(
  item: ClipboardItem,
  displayKind: DisplayKind,
  handlers: ActionHandlers
): ActionMenuItem[][] {
  const handlerByKey: Record<string, () => void> = {
    copy: handlers.onCopy,
    selectText: handlers.onSelectText,
    copyPlain: handlers.onCopyPlainText,
    openBrowser: handlers.onOpenInBrowser,
    saveImage: handlers.onSaveImage,
    saveFile: handlers.onSaveFile,
    resend: handlers.onResend,
    share: handlers.onShare,
    select: handlers.onSelect,
    delete: handlers.onDelete,
  };

  const descriptors = getClipboardCardActionDescriptors(item, displayKind);
  return GROUP_ORDER.map((group) =>
    descriptors
      .filter((d) => d.group === group)
      .map((d) => ({
        key: d.key,
        label: d.label,
        icon: d.androidIcon,
        destructive: d.destructive,
        onPress: handlerByKey[d.key],
      }))
  ).filter((group) => group.length > 0);
}
