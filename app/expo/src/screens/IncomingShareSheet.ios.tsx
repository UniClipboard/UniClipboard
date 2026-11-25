import { useCallback, useEffect, useRef, useState } from 'react';
import { Alert, AppState } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Image,
  ProgressView,
  Section,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  background,
  buttonStyle,
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
  opacity,
  padding,
  shapes,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, SheetHeader } from '@/components/ui';
import {
  iosProminentButtonModifiers,
  iosSecondaryButtonModifiers,
} from '@/components/ui/iosButtonStyles.ios';
import { parseEngineError, unicEngineNative } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import { iosColors, iosKindTints } from '@/theme/iosDesignTokens';

const TERTIARY_LABEL = iosColors?.tertiaryLabel ?? '#8E8E93';
const SELECTED_ROW_BACKGROUND = iosColors?.tertiarySystemFill ?? '#E5E5EA';
const SHEET_BACKGROUND = iosColors?.systemGroupedBackground ?? '#F2F2F7';

export function IncomingShareSheet() {
  const { t } = useTranslation();
  const devices = useEngineStore((s) => s.devices);
  const [items, setItems] = useState<
    Awaited<ReturnType<typeof unicEngineNative.listIncomingShares>>
  >([]);
  const [target, setTarget] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const pending = useRef(false);
  const request = useRef(0);
  const item = items[0];

  const reload = useCallback(async () => {
    const seq = ++request.current;
    try {
      const current = await unicEngineNative.listIncomingShares();
      if (seq === request.current) setItems(current);
    } catch {
      // A later foreground transition or native event retries the read.
    }
  }, []);

  useEffect(() => {
    const shares = unicEngineNative.addIncomingShareListener(() => void reload());
    const app = AppState.addEventListener('change', (state) => {
      if (state === 'active') void reload();
    });
    void reload();
    return () => {
      request.current++;
      shares.remove();
      app.remove();
    };
  }, [reload]);

  useEffect(() => setTarget(null), [item?.id]);

  const handleRefreshDevices = useCallback(async () => {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await useEngineStore.getState().refreshDevicesAndCandidates();
    } catch {
      // Keep last known devices snapshot.
    } finally {
      setRefreshing(false);
    }
  }, [refreshing]);

  const handle = async (action: 'save' | 'send' | 'dismiss') => {
    if (!item || pending.current || (action === 'send' && !target)) return;
    pending.current = true;
    setBusy(true);
    try {
      if (action === 'dismiss') {
        await unicEngineNative.dismissIncomingShare(item.id);
      } else {
        const submission = await unicEngineNative.commitIncomingShare(
          item.id,
          action === 'send' && target ? [target] : undefined
        );
        if (action === 'send' && submission.sync_disposition !== 'queued') {
          Alert.alert(
            t(
              submission.history_id
                ? 'share:incoming.savedWithoutSend'
                : 'share:incoming.sendNotStarted'
            )
          );
        } else if (action === 'save' && !submission.history_id) {
          Alert.alert(t('share:incoming.notSaved'));
        }
        void useEngineStore.getState().refreshHistory();
      }
      await reload();
    } catch (error) {
      Alert.alert(
        t('share:incoming.failed'),
        t(`errors:engine.${parseEngineError(error).code}`, {
          defaultValue: t('errors:engine.internal'),
        })
      );
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };

  const trustedDevices = devices.filter((device) => device.trust === 'trusted');

  return (
    <IosBottomSheet
      standalone
      visible={Boolean(item)}
      onDismiss={() => void handle('dismiss')}
      detents={['large']}
      interactiveDismissDisabled={busy}
      backgroundColor={SHEET_BACKGROUND}
    >
      {item ? (
        <VStack
          spacing={0}
          modifiers={[
            frame({ maxWidth: Infinity, maxHeight: Infinity }),
            background(SHEET_BACKGROUND),
          ]}
        >
          <SheetHeader title={t('share:incoming.title')} />
          <IosSheetForm modifiers={[frame({ maxWidth: Infinity, maxHeight: Infinity })]}>
            <Section title={`${t('share:send.title')} (${items.length})`}>
              <HStack
                spacing={12}
                alignment="center"
                modifiers={[
                  frame({ maxWidth: Infinity }),
                  padding({ vertical: 4 }),
                  listRowInsets({ top: 8, bottom: 8, leading: 16, trailing: 16 }),
                ]}
              >
                <HStack alignment="center" modifiers={[frame({ width: 40, height: 40 })]}>
                  <HStack
                    alignment="center"
                    modifiers={[
                      frame({ width: 36, height: 36 }),
                      background(
                        `${iosKindTints.text}22`,
                        shapes.roundedRectangle({ cornerRadius: 10 })
                      ),
                    ]}
                  >
                    <Image systemName="doc.text.fill" size={18} color={iosKindTints.text} />
                  </HStack>
                </HStack>
                <SwiftUIText
                  modifiers={[
                    font({ size: 15, weight: 'medium' }),
                    lineLimit(3),
                    foregroundStyle('primary'),
                    frame({ maxWidth: Infinity, alignment: 'leading' }),
                  ]}
                >
                  {item.label}
                </SwiftUIText>
              </HStack>
            </Section>

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
                      disabled(refreshing || busy),
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
                trustedDevices.map((device) => {
                  const selected = target === device.id;
                  const connected = device.bindings?.some((b) => b.connection === 'connected');
                  return (
                    <SwiftUIButton
                      key={device.id}
                      onPress={() => setTarget(selected ? null : device.id)}
                      modifiers={[
                        buttonStyle('plain'),
                        disabled(busy),
                        listRowInsets({ top: 0, bottom: 0, leading: 0, trailing: 0 }),
                        ...(selected ? [listRowBackground(SELECTED_ROW_BACKGROUND)] : []),
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
                            modifiers={[
                              font({ size: 16 }),
                              foregroundStyle('primary'),
                              lineLimit(1),
                            ]}
                          >
                            {device.name || device.reported_name || device.id}
                          </SwiftUIText>
                          <SwiftUIText
                            modifiers={[
                              font({ size: 12 }),
                              foregroundStyle('secondary'),
                              lineLimit(1),
                            ]}
                          >
                            {connected ? t('share:compose.connected') : t('share:compose.offline')}
                          </SwiftUIText>
                        </VStack>
                        <Spacer />
                        <Image
                          systemName={selected ? 'checkmark.circle.fill' : 'circle'}
                          size={22}
                          color={selected ? iosKindTints.text : TERTIARY_LABEL}
                        />
                      </HStack>
                    </SwiftUIButton>
                  );
                })
              )}
            </Section>
          </IosSheetForm>

          <VStack spacing={10} modifiers={[padding({ horizontal: 20, top: 10, bottom: 16 })]}>
            <SwiftUIButton
              testID="incoming-share-save"
              onPress={() => void handle('save')}
              modifiers={[
                ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                controlSize('large'),
                disabled(busy),
              ]}
            >
              <HStack spacing={8} modifiers={[frame({ minHeight: 48, maxWidth: Infinity })]}>
                <Spacer />
                {busy ? <ProgressView /> : null}
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {t('share:incoming.save')}
                </SwiftUIText>
                <Spacer />
              </HStack>
            </SwiftUIButton>

            {target ? (
              <SwiftUIButton
                testID="incoming-share-send"
                onPress={() => void handle('send')}
                modifiers={[
                  ...iosSecondaryButtonModifiers({ fullWidth: true }),
                  controlSize('large'),
                  disabled(busy),
                ]}
              >
                <HStack spacing={8} modifiers={[frame({ minHeight: 48, maxWidth: Infinity })]}>
                  <Spacer />
                  <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                    {t('share:incoming.send')}
                  </SwiftUIText>
                  <Spacer />
                </HStack>
              </SwiftUIButton>
            ) : null}
          </VStack>
        </VStack>
      ) : null}
    </IosBottomSheet>
  );
}
