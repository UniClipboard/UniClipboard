import { CircularProgressIndicator, LinearProgressIndicator } from '@expo/ui/jetpack-compose';
import { fillMaxWidth } from '@expo/ui/jetpack-compose/modifiers';
import type { ColorValue } from 'react-native';

export interface AppProgressIndicatorProps {
  variant?: 'circular' | 'linear';
  progress?: number;
  color?: ColorValue;
  trackColor?: ColorValue;
  fullWidth?: boolean;
}

export function AppProgressIndicator({
  variant = 'circular',
  progress,
  color,
  trackColor,
  fullWidth,
}: AppProgressIndicatorProps) {
  if (variant === 'linear') {
    return (
      <LinearProgressIndicator
        progress={progress}
        color={color}
        trackColor={trackColor}
        modifiers={fullWidth ? [fillMaxWidth()] : undefined}
      />
    );
  }
  return <CircularProgressIndicator progress={progress} color={color} trackColor={trackColor} />;
}
