import {
  Icon,
  IconButton,
  OutlinedTextField,
  Shape,
  Text,
  TextField,
  useMaterialColors,
  useNativeState,
} from '@expo/ui/jetpack-compose';
import type { TextFieldKeyboardType, TextFieldColors } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, testID as testIDModifier } from '@expo/ui/jetpack-compose/modifiers';
import { useCallback, useEffect, useRef, useState } from 'react';

const ICONS = {
  visibility: require('../../assets/icons/visibility.xml'),
  visibilityOff: require('../../assets/icons/visibility_off.xml'),
};

const FILLED_SHAPE = Shape.RoundedCorner({
  cornerRadii: { topStart: 16, topEnd: 16, bottomStart: 16, bottomEnd: 16 },
});

/**
 * Tonal, rounded text field look shared by every Android form: a surfaceContainer fill that
 * matches the grouped rows, 16dp corners and no indicator line (focus shows in the floating
 * label). Fields that cannot go through AppTextField (error state, leading icon, IME actions)
 * pass this to Material's `TextField` so they still look the same.
 */
export function useFilledTextFieldStyle(): { shape: typeof FILLED_SHAPE; colors: TextFieldColors } {
  const colors = useMaterialColors();
  return {
    shape: FILLED_SHAPE,
    colors: {
      focusedContainerColor: colors.surfaceContainer,
      unfocusedContainerColor: colors.surfaceContainer,
      disabledContainerColor: colors.surfaceContainer,
      errorContainerColor: colors.surfaceContainer,
      focusedIndicatorColor: 'transparent',
      unfocusedIndicatorColor: 'transparent',
      disabledIndicatorColor: 'transparent',
    },
  };
}

export interface AppTextFieldProps {
  /**
   * `filled` (default) is the app's tonal, rounded field; `outlined` is the stock Material
   * outline, for the rare place that needs it.
   */
  variant?: 'outlined' | 'filled';
  value: string;
  onChangeText: (value: string) => void;
  placeholder?: string;
  label?: string;
  disabled?: boolean;
  secure?: boolean;
  secureToggleLabel?: string;
  secureHideLabel?: string;
  keyboardType?: TextFieldKeyboardType;
  fullWidth?: boolean;
  colors?: TextFieldColors;
  testID?: string;
}

export function AppTextField({
  variant = 'filled',
  value,
  onChangeText,
  placeholder,
  label,
  disabled,
  secure,
  secureToggleLabel,
  secureHideLabel,
  keyboardType,
  fullWidth,
  colors: fieldColors,
  testID,
}: AppTextFieldProps) {
  const colors = useMaterialColors();
  const nativeValue = useNativeState(value);
  const [secureVisible, setSecureVisible] = useState(false);
  const latestNativeValue = useRef(value);
  useEffect(() => {
    if (value === latestNativeValue.current) return;
    latestNativeValue.current = value;
    nativeValue.set(value);
  }, [nativeValue, value]);
  const handleValueChange = useCallback(
    (nextValue: string) => {
      latestNativeValue.current = nextValue;
      onChangeText(nextValue);
    },
    [onChangeText]
  );

  const filled = variant === 'filled';
  const Field = filled ? TextField : OutlinedTextField;
  const filledStyle = useFilledTextFieldStyle();

  return (
    <Field
      value={nativeValue}
      onValueChange={handleValueChange}
      enabled={disabled !== undefined ? !disabled : undefined}
      singleLine
      visualTransformation={secure && !secureVisible ? 'password' : undefined}
      keyboardOptions={{ keyboardType: secure ? 'password' : keyboardType }}
      {...(filled ? { shape: filledStyle.shape } : {})}
      colors={fieldColors ?? (filled ? filledStyle.colors : undefined)}
      modifiers={[
        ...(fullWidth ? [fillMaxWidth()] : []),
        ...(testID ? [testIDModifier(testID)] : []),
      ]}
    >
      {label ? (
        <Field.Label>
          <Text>{label}</Text>
        </Field.Label>
      ) : null}
      {placeholder ? (
        <Field.Placeholder>
          <Text>{placeholder}</Text>
        </Field.Placeholder>
      ) : null}
      {secure && secureToggleLabel ? (
        <Field.TrailingIcon>
          <IconButton onClick={() => setSecureVisible((visible) => !visible)}>
            <Icon
              source={secureVisible ? ICONS.visibilityOff : ICONS.visibility}
              size={20}
              tint={colors.onSurfaceVariant}
              contentDescription={
                secureVisible ? secureHideLabel ?? secureToggleLabel : secureToggleLabel
              }
            />
          </IconButton>
        </Field.TrailingIcon>
      ) : null}
    </Field>
  );
}
