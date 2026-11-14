import { Toggle } from '@expo/ui/swift-ui';
import { disabled as disabledModifier, tint } from '@expo/ui/swift-ui/modifiers';
import type { ColorValue } from 'react-native';

export interface AppSwitchProps {
  value: boolean;
  onValueChange: (value: boolean) => void;
  disabled?: boolean;
  colors?: {
    checkedTrackColor?: ColorValue;
    uncheckedTrackColor?: ColorValue;
    checkedThumbColor?: ColorValue;
    uncheckedThumbColor?: ColorValue;
  };
}

export function AppSwitch({ value, onValueChange, disabled, colors }: AppSwitchProps) {
  const modifiers = [
    ...(disabled ? [disabledModifier(true)] : []),
    ...(colors?.checkedTrackColor ? [tint(colors.checkedTrackColor as string)] : []),
  ];
  return <Toggle isOn={value} onIsOnChange={onValueChange} modifiers={modifiers} />;
}
