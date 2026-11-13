import { HeaderCircleButton } from '@/screens/settings/ios/common';
import type { OnboardingCloseButtonProps } from './OnboardingCloseButton.types';

export function OnboardingCloseButton({ onPress, disabled, label }: OnboardingCloseButtonProps) {
  return (
    <HeaderCircleButton
      systemName="xmark"
      onPress={onPress}
      disabled={disabled}
      accessibilityLabel={label}
    />
  );
}
