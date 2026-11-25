import { useMemo, useState } from 'react';
import { Alert } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  controlSize,
  disabled,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  padding,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import {
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import { buildDeviceNameMap } from '@/engine/mapper';
import { useEngineStore } from '@/engine/store';
import type { ReceivedContent } from '@/engine/types';
import { IconToggleRow, settingsTileColors } from '@/screens/settings/ios/common';
import { formatFileSize } from '@/utils';
import { formatRelativeTime } from '@/utils/displayKind';

export function ReceiveResultsSheet({
  visible,
  onClose,
}: {
  visible: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const received = useEngineStore((s) => s.received);
  const devices = useEngineStore((s) => s.devices);
  const autoApplyReceived = useEngineStore((s) => s.autoApplyReceived);
  const [busyId, setBusyId] = useState<string | null>(null);

  const deviceNames = useMemo(() => buildDeviceNameMap(devices), [devices]);

  const describeReceivedContent = (entry: ReceivedContent): string => {
    const parts = entry.content?.parts ?? [];
    const fileParts = parts.filter((p) => p.kind === 'file');
    if (fileParts.length > 1) {
      return t('history:receiveQueue.filesCount', { count: fileParts.length });
    }
    if (fileParts.length === 1) {
      return fileParts[0].name || t('history:kind.file');
    }
    if (parts.some((p) => p.mime?.startsWith('image/'))) {
      return t('history:receiveQueue.imageContent');
    }
    if (parts.some((p) => p.mime === 'text/plain' || p.mime === 'text/html')) {
      return t('history:receiveQueue.textContent');
    }
    return t('history:receiveQueue.clipboardContent');
  };

  const applyStateLabel = (state: string): string => {
    switch (state) {
      case 'applied':
        return t('history:receiveQueue.state.applied');
      case 'applying':
        return t('history:receiveQueue.state.applying');
      case 'failed':
        return t('history:receiveQueue.state.failed');
      case 'deferred':
        return t('history:receiveQueue.state.deferred');
      default:
        return t('history:receiveQueue.state.pending');
    }
  };

  const handleApply = async (entry: ReceivedContent) => {
    if (busyId) return;
    setBusyId(entry.receipt_id);
    try {
      await useEngineStore.getState().applyReceivedToClipboard(entry.receipt_id);
    } catch (err) {
      Alert.alert(
        t('common:action.copy'),
        err instanceof Error ? err.message : t('shell:unavailable')
      );
    } finally {
      setBusyId(null);
    }
  };

  const handleDismiss = async (entry: ReceivedContent) => {
    if (busyId) return;
    setBusyId(entry.receipt_id);
    try {
      await useEngineStore.getState().dismissReceived(entry.receipt_id);
    } catch (err) {
      Alert.alert(
        t('common:action.close'),
        err instanceof Error ? err.message : t('shell:unavailable')
      );
    } finally {
      setBusyId(null);
    }
  };

  return (
    <IosBottomSheet standalone visible={visible} onDismiss={onClose} detents={['medium', 'large']}>
      <IosSheetPage
        title={t('history:receiveQueue.title')}
        rightSlots={[
          <SheetCircleButton
            key="close"
            testID="receive-results-close"
            systemName="xmark"
            accessibilityLabel={t('common:action.close')}
            onPress={onClose}
          />,
        ]}
      >
        <IosSheetForm>
          <Section>
            <IconToggleRow
              icon="arrow.down.doc"
              iconColor={settingsTileColors.green}
              label={t('history:receiveQueue.autoApply')}
              isOn={autoApplyReceived}
              onIsOnChange={(enabled) => {
                void useEngineStore
                  .getState()
                  .setAutoApplyReceived(enabled)
                  .catch((error) => {
                    Alert.alert(
                      t('settingsStorage:setFailed'),
                      error instanceof Error ? error.message : undefined
                    );
                  });
              }}
            />
          </Section>

          {received.length === 0 ? (
            <Section>
              <SwiftUIText modifiers={[foregroundStyle('secondary')]}>
                {t('history:receiveQueue.empty')}
              </SwiftUIText>
            </Section>
          ) : (
            received.map((entry) => {
              const sender = entry.peer_id
                ? (deviceNames.get(entry.peer_id) ?? entry.peer_id.slice(0, 8))
                : entry.channel_id || t('history:receiveQueue.remoteDevice');
              const isBusy = busyId === entry.receipt_id;
              return (
                <Section key={entry.receipt_id}>
                  <VStack alignment="leading" spacing={10} modifiers={[padding({ vertical: 4 })]}>
                    <HStack spacing={8} modifiers={[frame({ maxWidth: Infinity })]}>
                      <SwiftUIText modifiers={[font({ weight: 'semibold' }), lineLimit(1)]}>
                        {describeReceivedContent(entry)}
                      </SwiftUIText>
                      <Spacer />
                      <SwiftUIText modifiers={[font({ size: 12 }), foregroundStyle('secondary')]}>
                        {applyStateLabel(entry.apply_state)}
                      </SwiftUIText>
                    </HStack>
                    <SwiftUIText modifiers={[font({ size: 12 }), foregroundStyle('secondary')]}>
                      {t('history:receiveQueue.fromSender', {
                        sender,
                        time: formatRelativeTime(entry.received_at_ms),
                      })}
                      {entry.content?.total_size
                        ? ` · ${formatFileSize(entry.content.total_size)}`
                        : ''}
                    </SwiftUIText>
                    <HStack spacing={10} modifiers={[frame({ maxWidth: Infinity })]}>
                      <SwiftUIButton
                        onPress={() => void handleApply(entry)}
                        modifiers={[
                          ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                          controlSize('regular'),
                          disabled(isBusy),
                        ]}
                      >
                        <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                          <Spacer />
                          <SwiftUIText modifiers={[font({ size: 14, weight: 'semibold' })]}>
                            {t('history:receiveQueue.copyToClipboard')}
                          </SwiftUIText>
                          <Spacer />
                        </HStack>
                      </SwiftUIButton>
                      <SwiftUIButton
                        onPress={() => void handleDismiss(entry)}
                        modifiers={[
                          ...iosSecondaryButtonModifiers({ fullWidth: true }),
                          controlSize('regular'),
                          disabled(isBusy),
                        ]}
                      >
                        <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                          <Spacer />
                          <SwiftUIText modifiers={[font({ size: 14, weight: 'semibold' })]}>
                            {t('history:receiveQueue.dismiss')}
                          </SwiftUIText>
                          <Spacer />
                        </HStack>
                      </SwiftUIButton>
                    </HStack>
                  </VStack>
                </Section>
              );
            })
          )}
        </IosSheetForm>
      </IosSheetPage>
    </IosBottomSheet>
  );
}
