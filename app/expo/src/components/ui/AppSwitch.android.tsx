import { Switch } from '@expo/ui/jetpack-compose';
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
  return (
    <Switch
      value={value}
      onCheckedChange={onValueChange}
      enabled={disabled !== undefined ? !disabled : undefined}
      colors={colors}
    />
  );
}
