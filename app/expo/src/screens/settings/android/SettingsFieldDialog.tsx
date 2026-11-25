import { useState } from 'react';
import { Modal, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { useTheme } from '@/hooks/useTheme';

export interface AndroidSettingFieldEdit {
  id: string;
  title: string;
  value: string;
  placeholder?: string;
  numeric?: boolean;
  save: (value: string) => Promise<void>;
}

export function SettingsFieldDialog({
  edit,
  onClose,
}: {
  edit: AndroidSettingFieldEdit | null;
  onClose: () => void;
}) {
  if (!edit) return null;
  return <SettingsFieldDialogInner key={edit.id} edit={edit} onClose={onClose} />;
}

function SettingsFieldDialogInner({
  edit,
  onClose,
}: {
  edit: AndroidSettingFieldEdit;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const {
    theme: { colors },
  } = useTheme();
  const [value, setValue] = useState(edit.value);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  const save = async () => {
    if (saving) return;
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
    <Modal transparent animationType="fade" onRequestClose={saving ? undefined : onClose}>
      <View style={styles.backdrop}>
        <View
          accessibilityViewIsModal
          style={[styles.dialog, { backgroundColor: colors.surfaceHigh }]}
        >
          <Text accessibilityRole="header" style={[styles.title, { color: colors.textPrimary }]}>
            {edit.title}
          </Text>
          <TextInput
            testID={`${edit.id}-input`}
            accessibilityLabel={edit.title}
            value={value}
            onChangeText={setValue}
            placeholder={edit.placeholder}
            placeholderTextColor={colors.textSecondary}
            autoFocus
            editable={!saving}
            autoCorrect={false}
            autoCapitalize="none"
            keyboardType={edit.numeric ? 'number-pad' : 'default'}
            onSubmitEditing={() => void save()}
            style={[
              styles.input,
              {
                backgroundColor: colors.surfaceLowest,
                borderColor: colors.border,
                color: colors.textPrimary,
              },
            ]}
          />
          {error ? (
            <Text accessibilityRole="alert" style={[styles.error, { color: colors.error }]}>
              {error}
            </Text>
          ) : null}
          <View style={styles.buttons}>
            <Pressable
              accessibilityRole="button"
              disabled={saving}
              onPress={onClose}
              style={styles.button}
            >
              <Text style={[styles.buttonText, { color: colors.textSecondary }]}>
                {t('common:action.cancel')}
              </Text>
            </Pressable>
            <Pressable
              testID="setting-save"
              accessibilityRole="button"
              disabled={saving}
              onPress={() => void save()}
              style={styles.button}
            >
              <Text style={[styles.buttonText, { color: colors.accent }]}>
                {t('common:action.save')}
              </Text>
            </Pressable>
          </View>
        </View>
      </View>
    </Modal>
  );
}

const styles = StyleSheet.create({
  backdrop: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
    padding: 24,
    backgroundColor: 'rgba(0,0,0,0.45)',
  },
  dialog: {
    width: '100%',
    maxWidth: 420,
    borderRadius: 28,
    padding: 24,
    gap: 14,
  },
  title: {
    fontSize: 22,
    fontWeight: '500',
  },
  input: {
    paddingHorizontal: 14,
    minHeight: 52,
    borderRadius: 12,
    borderWidth: 1,
    fontSize: 16,
  },
  error: {
    fontSize: 13,
  },
  buttons: {
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: 8,
    marginTop: 4,
  },
  button: {
    paddingHorizontal: 14,
    minHeight: 44,
    justifyContent: 'center',
  },
  buttonText: {
    fontSize: 14,
    fontWeight: '600',
  },
});
