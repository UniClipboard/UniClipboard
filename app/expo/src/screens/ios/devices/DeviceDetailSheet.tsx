import { useEffect, useRef, useState } from 'react';
import {
  Alert,
  BottomSheet,
  Button as SwiftUIButton,
  Group,
  HStack,
  Image,
  List,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  type TextFieldRef,
  VStack,
} from '@expo/ui/swift-ui';
import {
  font,
  foregroundStyle,
  frame,
  listStyle,
  presentationDetents,
  presentationDragIndicator,
  scrollContentBackground,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosPageChromeProvider, IosSheetPage } from '@/components/ui';
import type { DeviceBinding, EngineDevice } from '@/engine/types';
import {
  HeaderCircleButton,
  IconToggleRow,
  SettingsNavRow,
  settingsTileColors,
} from '@/screens/settings/ios/common';
import { deviceDisplayName, engineDeviceStatus } from './deviceRows';

export interface DeviceDetailSheetProps {
  device: EngineDevice | null;
  onClose: () => void;
  onRename: (device: EngineDevice, nextName: string) => Promise<void>;
  onToggleSync: (binding: DeviceBinding, enabled: boolean) => Promise<void>;
  onUnpair: (device: EngineDevice) => Promise<void>;
}

/**
 * 设备详情底部弹层（对齐 v1 SpaceDeviceDetail.ios.tsx）。
 * 由 DevicesScreen 稳定宿主作为 NavigationStack 兄弟节点持有。
 */
export function DeviceDetailSheet({
  device,
  onClose,
  onRename,
  onToggleSync,
  onUnpair,
}: DeviceDetailSheetProps) {
  const { t } = useTranslation();
  const pair = (key: string, values?: Record<string, string>) =>
    t(`settingsSync:pairing.${key}`, values);
  const [name, setName] = useState('');
  const fieldRef = useRef<TextFieldRef>(null);
  const [saving, setSaving] = useState(false);
  const [confirmUnpair, setConfirmUnpair] = useState(false);

  useEffect(() => {
    if (device) {
      const initial = device.name || device.reported_name || '';
      setName(initial);
      void fieldRef.current?.setText(initial);
      setConfirmUnpair(false);
      setSaving(false);
    }
  }, [device]);

  const status = device ? engineDeviceStatus(device, pair) : null;
  const trimmedName = name.trim();
  const canSaveName = Boolean(device && trimmedName && trimmedName !== device.name && !saving);

  return (
    <BottomSheet
      isPresented={device !== null}
      onIsPresentedChange={(presented) => {
        if (!presented) onClose();
      }}
    >
      <Group modifiers={[presentationDetents(['large']), presentationDragIndicator('visible')]}>
        {device ? (
          <IosPageChromeProvider value={{ kind: 'sheet' }}>
            <IosSheetPage
              title={t('settingsSync:space.deviceDetail.title')}
              rightSlots={[
                <HeaderCircleButton
                  key="close"
                  systemName="xmark"
                  accessibilityLabel={t('common:action.close')}
                  onPress={onClose}
                />,
              ]}
            >
              <List modifiers={[listStyle('insetGrouped'), scrollContentBackground('hidden')]}>
                <Section>
                  <HStack
                    spacing={12}
                    alignment="center"
                    modifiers={[frame({ maxWidth: Infinity })]}
                  >
                    <Image
                      systemName="laptopcomputer"
                      size={32}
                      color={settingsTileColors.indigo}
                      modifiers={[frame({ width: 42, height: 42 })]}
                    />
                    <VStack alignment="leading" spacing={4}>
                      <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                        {deviceDisplayName(device)}
                      </SwiftUIText>
                      <SwiftUIText modifiers={[foregroundStyle('secondary')]}>
                        {status?.label ?? ''}
                      </SwiftUIText>
                    </VStack>
                  </HStack>
                </Section>

                <Section>
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <SwiftUIText>{t('settingsSync:space.deviceDetail.identity.label')}</SwiftUIText>
                    <Spacer />
                    <SwiftUIText modifiers={[foregroundStyle('secondary')]}>
                      {t('settingsSync:space.deviceDetail.identity.remote')}
                    </SwiftUIText>
                  </HStack>
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <SwiftUIText>
                      {t('settingsSync:space.deviceDetail.reachability.label')}
                    </SwiftUIText>
                    <Spacer />
                    <SwiftUIText modifiers={[foregroundStyle('secondary')]}>
                      {status?.label ?? ''}
                    </SwiftUIText>
                  </HStack>
                  <VStack alignment="leading" spacing={4}>
                    <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
                      {pair('fingerprint', { value: device.fingerprint })}
                    </SwiftUIText>
                  </VStack>
                </Section>

                <Section header={<SwiftUIText>{pair('rename')}</SwiftUIText>}>
                  <TextField
                    ref={fieldRef}
                    testID="devices-rename-input"
                    placeholder={pair('rename')}
                    onTextChange={setName}
                  />
                  <SettingsNavRow
                    icon="checkmark.circle.fill"
                    iconColor={settingsTileColors.blue}
                    title={t('common:action.save')}
                    disabled={!canSaveName}
                    showsChevron={false}
                    showsPressFeedback={false}
                    onPress={() => {
                      if (!canSaveName) return;
                      setSaving(true);
                      void onRename(device, trimmedName).finally(() => setSaving(false));
                    }}
                  />
                </Section>

                {device.bindings && device.bindings.length > 0 ? (
                  <Section header={<SwiftUIText>{t('settings:category.sync')}</SwiftUIText>}>
                    {device.bindings.map((binding) => (
                      <IconToggleRow
                        key={binding.id}
                        icon="arrow.triangle.2.circlepath"
                        iconColor={settingsTileColors.green}
                        label={pair(binding.sync_enabled ? 'pauseSync' : 'resumeSync')}
                        isOn={binding.sync_enabled}
                        onIsOnChange={(enabled) => void onToggleSync(binding, enabled)}
                      />
                    ))}
                  </Section>
                ) : null}

                <Section footer={<SwiftUIText>{t('settingsSync:space.devices.removeEffect')}</SwiftUIText>}>
                  <Alert
                    title={pair('unpair')}
                    isPresented={confirmUnpair}
                    onIsPresentedChange={(presented) => {
                      if (!presented) setConfirmUnpair(false);
                    }}
                  >
                    <Alert.Trigger>
                      <SettingsNavRow
                        icon="trash"
                        iconColor={settingsTileColors.red}
                        title={pair('unpair')}
                        destructive
                        showsChevron={false}
                        showsPressFeedback={false}
                        onPress={() => setConfirmUnpair(true)}
                      />
                    </Alert.Trigger>
                    <Alert.Actions>
                      <SwiftUIButton
                        label={pair('unpair')}
                        role="destructive"
                        onPress={() => {
                          setConfirmUnpair(false);
                          void onUnpair(device);
                        }}
                      />
                      <SwiftUIButton
                        label={t('common:action.cancel')}
                        role="cancel"
                        onPress={() => setConfirmUnpair(false)}
                      />
                    </Alert.Actions>
                    <Alert.Message>
                      <SwiftUIText>
                        {t('settingsSync:space.devices.removeConfirmNamed', {
                          device: deviceDisplayName(device),
                        })}
                      </SwiftUIText>
                    </Alert.Message>
                  </Alert>
                </Section>
              </List>
            </IosSheetPage>
          </IosPageChromeProvider>
        ) : null}
      </Group>
    </BottomSheet>
  );
}
