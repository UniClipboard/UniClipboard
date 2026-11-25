import { useEffect, useState } from 'react';
import {
  Alert,
  Button as SwiftUIButton,
  HStack,
  Image,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  useNativeState,
  VStack,
} from '@expo/ui/swift-ui';
import {
  font,
  foregroundStyle,
  frame,
  onSubmit,
  submitLabel,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import type { DeviceBinding, EngineDevice } from '@/engine/types';
import {
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
 * Native SwiftUI device detail sheet owned by `DevicesScreen.ios` as a sibling of `NavigationStack`.
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
  const nameState = useNativeState('');
  const [saving, setSaving] = useState(false);
  const [confirmUnpair, setConfirmUnpair] = useState(false);

  useEffect(() => {
    if (device) {
      const initial = device.name || device.reported_name || '';
      setName(initial);
      nameState.set(initial);
      setConfirmUnpair(false);
      setSaving(false);
    }
  }, [device, nameState]);

  const status = device ? engineDeviceStatus(device, pair) : null;
  const trimmedName = name.trim();
  const canSaveName = Boolean(device && trimmedName && trimmedName !== device.name && !saving);

  const handleRename = () => {
    if (!device || !canSaveName) return;
    setSaving(true);
    void onRename(device, trimmedName).finally(() => setSaving(false));
  };

  return (
    <IosBottomSheet
      visible={device !== null}
      onDismiss={onClose}
      detents={['large']}
      interactiveDismissDisabled={saving}
    >
      {device ? (
        <IosSheetPage
          title={t('settingsSync:space.deviceDetail.title')}
          rightSlots={[
            <SheetCircleButton
              key="close"
              systemName="xmark"
              accessibilityLabel={t('common:action.close')}
              disabled={saving}
              onPress={onClose}
            />,
          ]}
        >
          <IosSheetForm>
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
                key={device.id}
                testID="devices-rename-input"
                text={nameState}
                placeholder={pair('rename')}
                onTextChange={setName}
                modifiers={[submitLabel('done'), onSubmit(handleRename)]}
              />
              <SettingsNavRow
                icon="checkmark.circle.fill"
                iconColor={settingsTileColors.blue}
                title={t('common:action.save')}
                disabled={!canSaveName}
                showsChevron={false}
                showsPressFeedback={false}
                onPress={handleRename}
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
          </IosSheetForm>
        </IosSheetPage>
      ) : null}
    </IosBottomSheet>
  );
}
