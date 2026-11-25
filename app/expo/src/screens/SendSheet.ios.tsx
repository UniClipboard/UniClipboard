import { useCallback, useEffect, useState } from 'react';
import { Alert } from 'react-native';
import {
  Background,
  Button as SwiftUIButton,
  HStack,
  Image,
  ProgressView,
  Rectangle,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  useNativeState,
  VStack,
} from '@expo/ui/swift-ui';
import {
  accessibilityValue,
  aspectRatio,
  background,
  buttonStyle,
  clipShape,
  clipped,
  contentShape,
  controlSize,
  disabled,
  font,
  foregroundStyle,
  frame,
  labelStyle,
  lineLimit,
  listRowBackground,
  listRowInsets,
  onGeometryChange,
  opacity,
  padding,
  resizable,
  shapes,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';
import type { SFSymbol } from 'sf-symbols-typescript';

import { IosBottomSheet, IosSheetForm, SheetHeader } from '@/components/ui';
import {
  iosProminentButtonModifiers,
  iosSaturatedButtonPalette,
} from '@/components/ui/iosButtonStyles.ios';
import { parseEngineError } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import type { EngineDevice } from '@/engine/types';
import { useHistoryImagePreview } from '@/hooks/useHistoryImagePreview';
import { useTheme } from '@/hooks/useTheme';
import { SettingsNavRow, settingsTileColors } from '@/screens/settings/ios/common';
import { showUnavailable } from '@/shell/notice';
import { iosColors, iosKindTints } from '@/theme/iosDesignTokens';
import type { ClipboardItem } from '@/types/clipboard';
import { formatFileSize } from '@/utils';

const TERTIARY_LABEL = iosColors?.tertiaryLabel ?? '#8E8E93';
const IMAGE_PREVIEW_SIZE = 64;
const SELECTED_ROW_BACKGROUND = iosColors?.tertiarySystemFill ?? '#E5E5EA';
const SHEET_BACKGROUND = iosColors?.systemGroupedBackground ?? '#F2F2F7';

type TargetDeliveryPhase = 'idle' | 'sending' | 'success' | 'failed';

/**
 * Native iOS Send sheet aligned with upstream `ShareSendSheet.ios.tsx`.
 * Opens at large detent with insetGrouped content + device target sections and a pinned capsule footer.
 */
export function SendSheet({
  visible,
  onClose,
  targetItem,
  initialDraftText,
}: {
  visible: boolean;
  onClose: () => void;
  targetItem?: ClipboardItem | null;
  initialDraftText?: string | null;
}) {
  const { t } = useTranslation();
  const mode = useEngineStore((s) => s.mode);
  const devices = useEngineStore((s) => s.devices);
  const [draftText, setDraftText] = useState('');
  const draftState = useNativeState('');
  const [selectedDeviceIds, setSelectedDeviceIds] = useState<string[]>([]);
  const [phase, setPhase] = useState<TargetDeliveryPhase>('idle');
  const [refreshing, setRefreshing] = useState(false);

  const imagePreview = useHistoryImagePreview(targetItem ?? null, visible);

  useEffect(() => {
    if (visible) {
      const next = initialDraftText ?? '';
      setDraftText(next);
      draftState.set(next);
      setPhase('idle');
    } else {
      setSelectedDeviceIds([]);
      setPhase('idle');
    }
  }, [visible, initialDraftText, targetItem?.profileHash, draftState]);

  const errorMessage = (error: unknown) =>
    t(`errors:engine.${parseEngineError(error).code}`, {
      defaultValue: t('errors:engine.internal'),
    });

  const handleRefreshDevices = useCallback(async () => {
    if (refreshing || mode !== 'live') return;
    setRefreshing(true);
    try {
      await useEngineStore.getState().refreshDevicesAndCandidates();
    } catch {
      // Device list stays at last known snapshot on refresh failure.
    } finally {
      setRefreshing(false);
    }
  }, [refreshing, mode]);

  const pageTitle = targetItem ? t('home:detail.sendTo') : t('share:send.title');

  if (mode !== 'live') {
    return (
      <IosBottomSheet
        standalone
        visible={visible}
        onDismiss={onClose}
        detents={['large']}
        backgroundColor={SHEET_BACKGROUND}
      >
        <VStack
          spacing={0}
          modifiers={[
            frame({ maxWidth: Infinity, maxHeight: Infinity }),
            background(SHEET_BACKGROUND),
          ]}
        >
          <SheetHeader title={pageTitle} />
          <IosSheetForm modifiers={[frame({ maxWidth: Infinity, maxHeight: Infinity })]}>
            <Section
              header={<SwiftUIText>{t('share:send.devices')}</SwiftUIText>}
              footer={<SwiftUIText>{t('shell:unavailable')}</SwiftUIText>}
            >
              {(['demoMac', 'demoPhone'] as const).map((device) => (
                <SwiftUIButton
                  key={device}
                  testID={`send-${device}`}
                  onPress={showUnavailable}
                  modifiers={[
                    buttonStyle('plain'),
                    listRowInsets({ top: 0, bottom: 0, leading: 0, trailing: 0 }),
                  ]}
                >
                  <HStack
                    spacing={10}
                    alignment="center"
                    modifiers={[
                      frame({ maxWidth: Infinity, minHeight: 48 }),
                      padding({ horizontal: 16, vertical: 14 }),
                      contentShape(shapes.rectangle()),
                    ]}
                  >
                    <VStack
                      alignment="leading"
                      spacing={4}
                      modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}
                    >
                      <SwiftUIText
                        modifiers={[font({ size: 16 }), foregroundStyle('primary'), lineLimit(1)]}
                      >
                        {t(`settingsSync:pairing.${device}`)}
                      </SwiftUIText>
                      <SwiftUIText
                        modifiers={[font({ size: 12 }), foregroundStyle('secondary'), lineLimit(1)]}
                      >
                        {t('shell:disconnected')}
                      </SwiftUIText>
                    </VStack>
                    <Spacer />
                    <Image systemName="circle" size={22} color={TERTIARY_LABEL} />
                  </HStack>
                </SwiftUIButton>
              ))}
            </Section>
          </IosSheetForm>
          <SwiftUIButton
            testID="send-close"
            onPress={onClose}
            modifiers={[
              ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
              controlSize('large'),
              padding({ horizontal: 20, top: 10, bottom: 16 }),
            ]}
          >
            <HStack spacing={8} modifiers={[frame({ minHeight: 48, maxWidth: Infinity })]}>
              <Spacer />
              <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                {t('common:action.close')}
              </SwiftUIText>
              <Spacer />
            </HStack>
          </SwiftUIButton>
        </VStack>
      </IosBottomSheet>
    );
  }

  const trustedDevices = devices.filter((d) => d.trust === 'trusted');
  const isSending = phase === 'sending';
  const isDone = phase === 'success';
  const hasFailed = phase === 'failed';
  const locked = isSending || isDone;
  const targetIds = selectedDeviceIds.length > 0 ? selectedDeviceIds : undefined;

  const toggleDeviceSelection = (id: string) => {
    if (locked) return;
    setSelectedDeviceIds((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id]
    );
  };

  const handleResendTargetItem = async () => {
    if (!targetItem || isSending) return;
    if (isDone) {
      onClose();
      return;
    }
    setPhase('sending');
    try {
      await useEngineStore.getState().resendHistoryItem(targetItem.profileHash, targetIds);
      setPhase('success');
    } catch (err) {
      setPhase('failed');
      Alert.alert(t('home:detail.sendTo'), errorMessage(err));
    }
  };

  const handleCommitText = async () => {
    const trimmed = draftText.trim();
    if (isDone) {
      onClose();
      return;
    }
    if (!trimmed || isSending) return;
    setPhase('sending');
    try {
      await useEngineStore.getState().commitText(trimmed, targetIds);
      setDraftText('');
      draftState.set('');
      setPhase('success');
    } catch (err) {
      setPhase('failed');
      Alert.alert(t('home:detail.sendTo'), errorMessage(err));
    }
  };

  const handleReadClipboard = async () => {
    if (isSending) return;
    setPhase('sending');
    try {
      const res = await useEngineStore.getState().readSystemClipboardAndCommit();
      if (res.skipped) {
        setPhase('idle');
        Alert.alert(t('share:compose.clipboard'), t('share:compose.emptyClipboard'));
      } else {
        onClose();
      }
    } catch (err) {
      setPhase('idle');
      Alert.alert(t('share:compose.readClipboard'), errorMessage(err));
    }
  };

  const handlePickFile = async () => {
    if (isSending) return;
    setPhase('sending');
    try {
      const res = await useEngineStore.getState().pickAndCommitFile(targetIds);
      if (!res.cancelled) {
        onClose();
      } else {
        setPhase('idle');
      }
    } catch (err) {
      setPhase('idle');
      Alert.alert(t('share:compose.sendFile'), errorMessage(err));
    }
  };

  const canSend = targetItem
    ? trustedDevices.length > 0
    : Boolean(draftText.trim());
  const footerEnabled = isDone || hasFailed || canSend;
  const footerLabel = isSending
    ? t('share:send.sending')
    : isDone
      ? t('share:send.done')
      : hasFailed
        ? t('share:send.retry')
        : targetItem
          ? t('share:send.sendAll')
          : t('share:compose.commitText');

  return (
    <IosBottomSheet
      standalone
      visible={visible}
      onDismiss={onClose}
      detents={['large']}
      interactiveDismissDisabled={isSending}
      backgroundColor={SHEET_BACKGROUND}
    >
      <VStack
        spacing={0}
        modifiers={[
          frame({ maxWidth: Infinity, maxHeight: Infinity }),
          background(SHEET_BACKGROUND),
        ]}
      >
        <SheetHeader title={pageTitle} />
        <IosSheetForm modifiers={[frame({ maxWidth: Infinity, maxHeight: Infinity })]}>
          {targetItem ? (
            <Section title={`${t('share:send.title')} (1)`}>
              <ItemPreviewRow item={targetItem} imageUri={imagePreview.uri} />
            </Section>
          ) : (
            <>
              <Section title={t('share:send.title')}>
                <TextField
                  testID="send-compose-input"
                  text={draftState}
                  placeholder={t('share:compose.textHint')}
                  axis="vertical"
                  onTextChange={setDraftText}
                  modifiers={[lineLimit({ min: 3, max: 6 })]}
                />
              </Section>
              <Section>
                <SettingsNavRow
                  testID="send-read-clipboard"
                  icon="doc.on.clipboard"
                  iconColor={settingsTileColors.orange}
                  title={t('share:compose.readClipboard')}
                  disabled={isSending}
                  showsChevron={false}
                  showsPressFeedback={false}
                  onPress={() => void handleReadClipboard()}
                />
                <SettingsNavRow
                  testID="send-pick-file"
                  icon="folder"
                  iconColor={settingsTileColors.blue}
                  title={t('share:compose.chooseFile')}
                  disabled={isSending}
                  showsChevron={false}
                  showsPressFeedback={false}
                  onPress={() => void handlePickFile()}
                />
              </Section>
            </>
          )}

          <Section
            header={
              <HStack alignment="center" modifiers={[frame({ maxWidth: Infinity })]}>
                <SwiftUIText>{t('share:send.devices')}</SwiftUIText>
                <Spacer />
                <SwiftUIButton
                  label={t('common:action.refresh')}
                  systemImage="arrow.clockwise"
                  onPress={() => void handleRefreshDevices()}
                  modifiers={[
                    buttonStyle('plain'),
                    labelStyle('iconOnly'),
                    frame({ minWidth: 44, minHeight: 44 }),
                    disabled(refreshing || locked),
                    opacity(refreshing ? 0.38 : 1),
                  ]}
                />
              </HStack>
            }
          >
            {refreshing && trustedDevices.length === 0 ? (
              <ProgressView />
            ) : trustedDevices.length === 0 ? (
              <SwiftUIText
                modifiers={[
                  foregroundStyle('secondary'),
                  font({ size: 14 }),
                  padding({ vertical: 8 }),
                ]}
              >
                {t('share:send.noDevices')}
              </SwiftUIText>
            ) : (
              trustedDevices.map((dev) => {
                const selected = selectedDeviceIds.includes(dev.id);
                const targeted =
                  selectedDeviceIds.length === 0 || selected;
                return (
                  <SendTargetRow
                    key={dev.id}
                    device={dev}
                    selected={selected}
                    phase={targeted ? phase : 'idle'}
                    locked={locked}
                    onToggle={toggleDeviceSelection}
                  />
                );
              })
            )}
          </Section>
        </IosSheetForm>

        <SwiftUIButton
          testID={targetItem ? 'send-resend-item' : 'send-commit-text'}
          onPress={() => void (targetItem ? handleResendTargetItem() : handleCommitText())}
          modifiers={[
            ...iosProminentButtonModifiers(
              isDone ? iosSaturatedButtonPalette(iosKindTints.image) : undefined,
              { fullWidth: true }
            ),
            controlSize('large'),
            disabled(!footerEnabled || isSending),
            padding({ horizontal: 20, top: 10, bottom: 16 }),
          ]}
        >
          <HStack spacing={8} modifiers={[frame({ minHeight: 48, maxWidth: Infinity })]}>
            <Spacer />
            {isSending ? <ProgressView /> : null}
            {isDone ? (
              <Image systemName="checkmark.circle.fill" size={18} color="#FFFFFF" />
            ) : null}
            <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>{footerLabel}</SwiftUIText>
            <Spacer />
          </HStack>
        </SwiftUIButton>
      </VStack>
    </IosBottomSheet>
  );
}

function ItemPreviewRow({
  item,
  imageUri,
}: {
  item: ClipboardItem;
  imageUri?: string;
}) {
  const { t } = useTranslation();
  const targetType = t(`errors:contentType.${item.type.toLowerCase()}`, {
    defaultValue: t('errors:contentType.unknown'),
  });
  const displayTitle = item.dataName || item.text?.trim() || `[${targetType}]`;
  const sizeLabel = item.size ? formatFileSize(item.size) : '';
  const metaText =
    item.type === 'Text'
      ? sizeLabel || targetType
      : sizeLabel
        ? `${targetType} · ${sizeLabel}`
        : targetType;

  return (
    <HStack
      spacing={12}
      alignment="center"
      modifiers={[
        frame({ maxWidth: Infinity }),
        padding({ vertical: 4 }),
        listRowInsets({ top: 8, bottom: 8, leading: 16, trailing: 16 }),
      ]}
    >
      <ItemLeading item={item} imageUri={imageUri} />
      <VStack
        alignment="leading"
        spacing={2}
        modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}
      >
        <SwiftUIText
          modifiers={[
            font({ size: 15, weight: 'medium' }),
            lineLimit(1),
            foregroundStyle('primary'),
          ]}
        >
          {displayTitle}
        </SwiftUIText>
        <SwiftUIText modifiers={[font({ size: 12 }), lineLimit(1), foregroundStyle('secondary')]}>
          {metaText}
        </SwiftUIText>
      </VStack>
    </HStack>
  );
}

function ItemLeading({ item, imageUri }: { item: ClipboardItem; imageUri?: string }) {
  if (item.type === 'Image' && imageUri) {
    return (
      <Image
        uiImage={imageUri}
        modifiers={[
          resizable(),
          aspectRatio({ contentMode: 'fit' }),
          frame({ width: IMAGE_PREVIEW_SIZE, height: IMAGE_PREVIEW_SIZE }),
          clipped(),
          clipShape('roundedRectangle', 8),
        ]}
      />
    );
  }

  let icon: SFSymbol = 'doc.fill';
  let tintColor: string = iosKindTints.file;
  if (item.type === 'Text') {
    icon = 'doc.text.fill';
    tintColor = iosKindTints.text;
  } else if (item.type === 'Image') {
    icon = 'photo.fill';
    tintColor = iosKindTints.image;
  } else if (item.type === 'Group') {
    icon = 'folder.fill';
    tintColor = iosKindTints.group;
  }

  return (
    <HStack alignment="center" modifiers={[frame({ width: 40, height: 40 })]}>
      <HStack
        alignment="center"
        modifiers={[
          frame({ width: 36, height: 36 }),
          background(`${tintColor}22`, shapes.roundedRectangle({ cornerRadius: 10 })),
        ]}
      >
        <Image systemName={icon} size={18} color={tintColor} />
      </HStack>
    </HStack>
  );
}

function SendTargetRow({
  device,
  selected,
  phase,
  locked,
  onToggle,
}: {
  device: EngineDevice;
  selected: boolean;
  phase: TargetDeliveryPhase;
  locked: boolean;
  onToggle: (id: string) => void;
}) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const [width, setWidth] = useState(0);
  const connected = device.bindings?.some((b) => b.connection === 'connected');
  const displayName = device.name || device.reported_name || device.id.slice(0, 8);

  const statusText =
    phase === 'sending'
      ? t('share:send.targetState.sending')
      : phase === 'success'
        ? t('share:send.targetState.success')
        : phase === 'failed'
          ? t('share:send.targetState.failed')
          : connected
            ? t('share:compose.connected')
            : t('share:compose.offline');

  const fillFraction = phase === 'success' || phase === 'failed' ? 1 : 0;
  const fillColor =
    phase === 'success'
      ? theme.colors.successContainer
      : phase === 'failed'
        ? theme.colors.warningContainer
        : theme.colors.infoContainer;

  return (
    <SwiftUIButton
      onPress={() => onToggle(device.id)}
      modifiers={[
        buttonStyle('plain'),
        disabled(locked),
        accessibilityValue(
          [selected ? 'selected' : 'not selected', statusText].filter(Boolean).join(', ')
        ),
        listRowInsets({ top: 0, bottom: 0, leading: 0, trailing: 0 }),
        ...(selected ? [listRowBackground(SELECTED_ROW_BACKGROUND)] : []),
      ]}
    >
      <Background alignment="leading">
        <HStack
          spacing={10}
          alignment="center"
          modifiers={[
            frame({ maxWidth: Infinity, minHeight: 48 }),
            padding({ horizontal: 16, vertical: 14 }),
            contentShape(shapes.rectangle()),
            onGeometryChange((layout) => setWidth(layout.width)),
          ]}
        >
          <VStack
            alignment="leading"
            spacing={4}
            modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}
          >
            <SwiftUIText
              modifiers={[font({ size: 16 }), foregroundStyle('primary'), lineLimit(1)]}
            >
              {displayName}
            </SwiftUIText>
            <SwiftUIText
              modifiers={[font({ size: 12 }), foregroundStyle('secondary'), lineLimit(1)]}
            >
              {statusText}
            </SwiftUIText>
          </VStack>
          <Spacer />
          {phase === 'success' ? (
            <Image systemName="checkmark.circle.fill" size={22} color={theme.colors.success} />
          ) : phase === 'failed' ? (
            <Image systemName="exclamationmark.circle" size={22} color={theme.colors.warning} />
          ) : phase === 'sending' ? (
            <ProgressView />
          ) : (
            <Image
              systemName={selected ? 'checkmark.circle.fill' : 'circle'}
              size={22}
              color={selected ? iosKindTints.text : TERTIARY_LABEL}
            />
          )}
        </HStack>
        <Background.Content>
          <Rectangle
            modifiers={[
              frame({ width: width * fillFraction, maxHeight: Infinity }),
              foregroundStyle(fillColor),
            ]}
          />
        </Background.Content>
      </Background>
    </SwiftUIButton>
  );
}
