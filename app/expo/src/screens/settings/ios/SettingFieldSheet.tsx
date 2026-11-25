import { useEffect, useRef, useState } from 'react';
import {
  BottomSheet,
  Button as SwiftUIButton,
  Group,
  HStack,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  type TextFieldRef,
  VStack,
} from '@expo/ui/swift-ui';
import {
  controlSize,
  disabled,
  font,
  foregroundStyle,
  frame,
  padding,
  presentationDetents,
  presentationDragIndicator,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosPageChromeProvider, IosSheetForm, IosSheetPage } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import { HeaderCircleButton, settingsTileColors } from './common';
import type { IosSettingFieldEdit } from './types';

/**
 * 原生 SwiftUI 表单项编辑弹层（由 SettingsScreen.ios.tsx 稳定宿主作为 NavigationStack 兄弟节点持有）。
 */
export function SettingFieldSheet({
  edit,
  onClose,
}: {
  edit: IosSettingFieldEdit | null;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState('');
  const fieldRef = useRef<TextFieldRef>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (edit) {
      setValue(edit.value);
      void fieldRef.current?.setText(edit.value);
      setSaving(false);
      setError('');
    }
  }, [edit]);

  const handleSave = async () => {
    if (!edit || saving) return;
    setSaving(true);
    setError('');
    try {
      await edit.save(value);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : t('settingsStorage:setFailed'));
      setSaving(false);
    }
  };

  return (
    <BottomSheet
      isPresented={edit !== null}
      onIsPresentedChange={(presented) => {
        if (!presented && !saving) onClose();
      }}
    >
      <Group modifiers={[presentationDetents(['medium']), presentationDragIndicator('visible')]}>
        {edit ? (
          <IosPageChromeProvider value={{ kind: 'sheet' }}>
            <IosSheetPage
              title={edit.title}
              rightSlots={[
                <HeaderCircleButton
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
                  <TextField
                    key={edit.id}
                    ref={fieldRef}
                    testID={`${edit.id}-input`}
                    placeholder={edit.placeholder ?? edit.title}
                    autoFocus
                    onTextChange={setValue}
                  />
                </Section>
                {error ? (
                  <Section>
                    <SwiftUIText modifiers={[foregroundStyle(settingsTileColors.red)]}>
                      {error}
                    </SwiftUIText>
                  </Section>
                ) : null}
              </IosSheetForm>
              <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
                <SwiftUIButton
                  testID="setting-save"
                  onPress={() => void handleSave()}
                  modifiers={[
                    ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                    controlSize('large'),
                    disabled(saving),
                  ]}
                >
                  <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                    <Spacer />
                    <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                      {t('common:action.save')}
                    </SwiftUIText>
                    <Spacer />
                  </HStack>
                </SwiftUIButton>
              </VStack>
            </IosSheetPage>
          </IosPageChromeProvider>
        ) : null}
      </Group>
    </BottomSheet>
  );
}
