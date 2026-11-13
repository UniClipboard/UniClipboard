import {
  View,
  Text,
  TextInput,
  StyleSheet,
  PlatformColor,
  DynamicColorIOS,
  type KeyboardTypeOptions,
} from 'react-native';

export interface AppTextFieldProps {
  value: string;
  onChangeText: (value: string) => void;
  placeholder?: string;
  label?: string;
  disabled?: boolean;
  secure?: boolean;
  keyboardType?: string;
  fullWidth?: boolean;
  colors?: unknown;
}

const KEYBOARD_MAP: Record<string, KeyboardTypeOptions> = {
  text: 'default',
  number: 'number-pad',
  decimal: 'decimal-pad',
  email: 'email-address',
  phone: 'phone-pad',
  uri: 'url',
};

// TODO: replace with SwiftUI TextField / SecureField.
export function AppTextField({
  value,
  onChangeText,
  placeholder,
  label,
  disabled,
  secure,
  keyboardType,
  fullWidth,
}: AppTextFieldProps) {
  return (
    <View style={fullWidth ? styles.fullWidth : undefined}>
      {label ? <Text style={styles.label}>{label}</Text> : null}
      <TextInput
        value={value}
        onChangeText={onChangeText}
        placeholder={placeholder}
        editable={disabled === undefined ? true : !disabled}
        secureTextEntry={secure}
        keyboardType={keyboardType ? KEYBOARD_MAP[keyboardType] ?? 'default' : 'default'}
        style={styles.input}
      />
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
  input: {
    borderWidth: 1,
    borderColor: PlatformColor('separator'),
    borderRadius: 8,
    paddingVertical: 10,
    paddingHorizontal: 12,
    fontSize: 16,
    color: PlatformColor('label'),
  },
});
