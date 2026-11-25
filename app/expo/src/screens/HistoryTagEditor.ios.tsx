import { useEffect, useState } from 'react';
import { Alert } from 'react-native';
import {
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
  accessibilityLabel,
  buttonStyle,
  controlSize,
  disabled,
  font,
  frame,
  onSubmit,
  padding,
  submitLabel,
} from '@expo/ui/swift-ui/modifiers';
import { useTranslation } from 'react-i18next';

import { IosBottomSheet, IosSheetForm, IosSheetPage, SheetCircleButton } from '@/components/ui';
import { iosProminentButtonModifiers } from '@/components/ui/iosButtonStyles.ios';
import { parseEngineError } from '@/engine/nativeModule';
import { useEngineStore } from '@/engine/store';
import { SettingsNavRow, settingsTileColors } from '@/screens/settings/ios/common';
import type { ClipboardItem } from '@/types/clipboard';

const reserved = new Set(['favorite', 'starred', 'pinned']);

export function HistoryTagEditor({
  item,
  onClose,
}: {
  item: ClipboardItem | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('history');
  const [draft, setDraft] = useState('');
  const draftState = useNativeState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft('');
    draftState.set('');
    setSaving(false);
  }, [item?.profileHash, draftState]);

  const tag = draft.trim();
  const tags = item?.tags?.filter((value) => !reserved.has(value)) ?? [];
  const canAdd =
    Boolean(item) &&
    !saving &&
    Boolean(tag) &&
    Array.from(tag).length <= 64 &&
    !reserved.has(tag) &&
    !tags.includes(tag);

  const update = async (value: string, present: boolean) => {
    if (!item || saving) return;
    setSaving(true);
    try {
      await useEngineStore.getState().setHistoryTag(item.profileHash, value, present);
      if (present) {
        setDraft('');
        draftState.set('');
      }
    } catch (error) {
      Alert.alert(
        t('tags.edit'),
        t(`errors:engine.${parseEngineError(error).code}`, {
          defaultValue: t('errors:engine.internal'),
        })
      );
    } finally {
      setSaving(false);
    }
  };

  return (
    <IosBottomSheet
      standalone
      visible={item !== null}
      onDismiss={onClose}
      detents={['medium', 'large']}
      interactiveDismissDisabled={saving}
    >
      {item ? (
        <IosSheetPage
          title={t('tags.edit')}
          rightSlots={[
            <SheetCircleButton
              key="close"
              testID="history-tag-close"
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
                key={item.profileHash}
                testID="history-tag-input"
                text={draftState}
                placeholder={t('tags.name')}
                onTextChange={setDraft}
                modifiers={[
                  submitLabel('done'),
                  onSubmit(() => {
                    if (canAdd) void update(tag, true);
                  }),
                ]}
              />
              <SettingsNavRow
                testID="history-tag-add"
                icon="plus.circle.fill"
                iconColor={settingsTileColors.blue}
                title={t('tags.add')}
                disabled={!canAdd}
                showsChevron={false}
                showsPressFeedback={false}
                onPress={() => {
                  if (canAdd) void update(tag, true);
                }}
              />
            </Section>

            {tags.length > 0 ? (
              <Section header={<SwiftUIText>{t('tags.filter')}</SwiftUIText>}>
                {tags.map((value) => (
                  <HStack
                    key={value}
                    spacing={12}
                    alignment="center"
                    modifiers={[frame({ maxWidth: Infinity })]}
                  >
                    <Image systemName="tag.fill" size={14} color={settingsTileColors.blue} />
                    <SwiftUIText>{value}</SwiftUIText>
                    <Spacer />
                    <SwiftUIButton
                      onPress={() => void update(value, false)}
                      modifiers={[
                        buttonStyle('plain'),
                        disabled(saving),
                        accessibilityLabel(t('tags.remove', { tag: value })),
                      ]}
                    >
                      <Image
                        systemName="minus.circle.fill"
                        size={20}
                        color={settingsTileColors.red}
                      />
                    </SwiftUIButton>
                  </HStack>
                ))}
              </Section>
            ) : null}
          </IosSheetForm>

          <VStack modifiers={[padding({ horizontal: 20, top: 8, bottom: 20 })]}>
            <SwiftUIButton
              onPress={onClose}
              modifiers={[
                ...iosProminentButtonModifiers(undefined, { fullWidth: true }),
                controlSize('large'),
                disabled(saving),
              ]}
            >
              <HStack modifiers={[frame({ maxWidth: Infinity })]}>
                <Spacer />
                <SwiftUIText modifiers={[font({ weight: 'semibold' })]}>
                  {t('common:action.done')}
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
