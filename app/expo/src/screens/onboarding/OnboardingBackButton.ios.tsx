import { StyleSheet } from 'react-native';
import { AppHost } from '@/components/ui';
import { HeaderCircleButton } from '@/screens/settings/ios/common';

export interface OnboardingBackButtonProps {
  onPress: () => void;
  disabled?: boolean;
  label: string;
}

export function OnboardingBackButton({ onPress, disabled, label }: OnboardingBackButtonProps) {
  return (
    <AppHost style={s.host}>
      <HeaderCircleButton
        systemName="chevron.left"
        onPress={onPress}
        disabled={disabled}
        accessibilityLabel={label}
      />
    </AppHost>
  );
}

const s = StyleSheet.create({
  host: { width: 42, height: 42 },
});
