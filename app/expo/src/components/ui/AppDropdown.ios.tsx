import { ActionSheetIOS, Pressable, Text, View, StyleSheet, PlatformColor } from 'react-native';
import { useTranslation } from 'react-i18next';

export interface AppDropdownOption<T extends string = string> {
  label: string;
  value: T;
}

export interface AppDropdownProps<T extends string = string> {
  options: AppDropdownOption<T>[];
  selectedValue?: T;
  onSelect: (value: T) => void;
  placeholder?: string;
  label?: string;
  disabled?: boolean;
  fullWidth?: boolean;
}

export function AppDropdown<T extends string = string>({
  options,
  selectedValue,
  onSelect,
  placeholder,
  label,
  disabled,
  fullWidth,
}: AppDropdownProps<T>) {
  const { t } = useTranslation('history');
  const selectedLabel = options.find((o) => o.value === selectedValue)?.label ?? placeholder ?? '';

  const openSheet = () => {
    if (disabled) return;
    const cancelButtonIndex = options.length;
    ActionSheetIOS.showActionSheetWithOptions(
      {
        options: [...options.map((o) => o.label), t('action.cancel', { ns: 'common' })],
        cancelButtonIndex,
        title: label,
      },
      (index) => {
        if (index != null && index < options.length) {
          onSelect(options[index].value);
        }
      }
    );
  };

  return (
    <View style={fullWidth ? styles.fullWidth : undefined}>
      {label ? <Text style={styles.label}>{label}</Text> : null}
      <Pressable
        onPress={openSheet}
        disabled={disabled}
        style={[styles.field, disabled ? styles.disabled : null]}
      >
        <Text style={[styles.value, !selectedValue ? styles.placeholder : null]}>
          {selectedLabel}
        </Text>
        <Text style={styles.chevron}>⌄</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  fullWidth: {
    alignSelf: 'stretch',
  },
  label: {
    fontSize: 13,
    color: PlatformColor('secondaryLabel'),
    marginBottom: 4,
  },
  field: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    borderWidth: 1,
    borderColor: PlatformColor('separator'),
    borderRadius: 8,
    paddingVertical: 10,
    paddingHorizontal: 12,
  },
  disabled: {
    opacity: 0.5,
  },
  value: {
    fontSize: 16,
    color: PlatformColor('label'),
    flexShrink: 1,
  },
  placeholder: {
    color: PlatformColor('placeholderText'),
  },
  chevron: {
    fontSize: 18,
    color: PlatformColor('secondaryLabel'),
    marginLeft: 8,
  },
});
