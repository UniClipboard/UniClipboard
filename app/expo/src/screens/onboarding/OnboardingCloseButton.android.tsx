import { Icon, IconButton } from '@expo/ui/jetpack-compose';
import { useTheme } from '@/hooks/useTheme';
import type { OnboardingCloseButtonProps } from './OnboardingCloseButton.types';

export function OnboardingCloseButton({ onPress, disabled, label }: OnboardingCloseButtonProps) {
  const { theme } = useTheme();
  return (
    <IconButton onClick={onPress} enabled={!disabled}>
      <Icon
        source={require('../../assets/icons/close.xml')}
        size={22}
        tint={theme.colors.textSecondary}
        contentDescription={label}
      />
    </IconButton>
  );
}
