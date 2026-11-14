import React, { useRef } from 'react';
import { Modal } from 'react-native';
import { ConnectedMessageToast } from './ConnectedMessageToast';
import { ClipboardDetailPage } from './android/ClipboardDetailPage';
import { DETAIL_TOOLBAR_HEIGHT, TOOLBAR_MARGIN } from './android/DetailFloatingToolbar';
import type { ClipboardItem } from '@/types/clipboard';
import type { ClipboardDetailModalProps } from './ClipboardDetailModal.types';

/**
 * Android 剪贴板详情:全屏页面,从底部滑入,系统返回键 / 预测性返回关闭。
 * 退场动画期间 item 可能已清空(关闭、删除),保留最后一条继续渲染直到滑出。
 */
export function ClipboardDetailModal({ visible, onDismiss, c, item }: ClipboardDetailModalProps) {
  const current = item === undefined ? c.detailItem : item;
  const lastItemRef = useRef<ClipboardItem | null>(null);
  if (current) lastItemRef.current = current;
  const shown = current ?? lastItemRef.current;

  return (
    <Modal
      visible={visible && shown != null}
      animationType="slide"
      statusBarTranslucent
      navigationBarTranslucent
      supportedOrientations={['portrait', 'landscape']}
      onRequestClose={onDismiss}
    >
      {shown ? <ClipboardDetailPage c={c} item={shown} onClose={onDismiss} /> : null}
      {/* Modal 在独立窗口,主树的 snackbar 会被盖住;抬到浮动工具栏之上 */}
      <ConnectedMessageToast
        bottomOffset={c.insets.bottom + TOOLBAR_MARGIN + DETAIL_TOOLBAR_HEIGHT + 12}
      />
    </Modal>
  );
}
