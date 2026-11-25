import { useEffect, useState } from 'react';
import {
  Button as SwiftUIButton,
  HStack,
  Section,
  Spacer,
  Text as SwiftUIText,
  TextField,
  useNativeState,
  VStack,
} from '@expo/ui/swift-ui';
import {
  controlSize,
  disabled,
  font,
  foregroundStyle,
  frame,
  onSubmit,
  padding,
  submitLabel,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import { settingsTileColors } from './common';
import type { IosSettingFieldEdit } from './types';

/**
 * Native SwiftUI field editor sheet owned by `SettingsScreen.ios` as a sibling of `NavigationStack`.
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
  const valueState = useNativeState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (edit) {
      setValue(edit.value);
      valueState.set(edit.value);
      setSaving(false);
      setError('');
    }
  }, [edit, valueState]);

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
    <IosBottomSheet
      visible={edit !== null}
      onDismiss={onClose}
      detents={['medium', 'large']}
      interactiveDismissDisabled={saving}
    >
      {edit ? (
        <IosSheetPage
          title={edit.title}
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
              <TextField
                key={edit.id}
                testID={`${edit.id}-input`}
                text={valueState}
                placeholder={edit.placeholder ?? edit.title}
                autoFocus
                onTextChange={setValue}
                modifiers={[submitLabel('done'), onSubmit(() => void handleSave())]}
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
      ) : null}
    </IosBottomSheet>
  );
}
