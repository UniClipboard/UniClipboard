import { M3IconButton } from '@/components/android/M3IconButton';
import { useTheme } from '@/hooks/useTheme';
import type { OnboardingBackButtonProps } from './OnboardingBackButton.types';

export function OnboardingBackButton({ onPress, disabled, label }: OnboardingBackButtonProps) {
  const { theme } = useTheme();
  return (
    <M3IconButton
      icon="arrow-back"
      onPress={onPress}
      disabled={disabled}
      accessibilityLabel={label}
      colors={theme.colors}
      iconColor={theme.colors.textPrimary}
    />
  );
}
