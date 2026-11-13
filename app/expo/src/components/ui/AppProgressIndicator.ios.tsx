import { ProgressView } from '@expo/ui/swift-ui';
import { frame, tint } from '@expo/ui/swift-ui/modifiers';
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
  fullWidth,
}: AppProgressIndicatorProps) {
  const modifiers = [
    ...(variant === 'linear' && fullWidth ? [frame({ maxWidth: Infinity })] : []),
    ...(color ? [tint(color as string)] : []),
  ];
  return <ProgressView value={progress} modifiers={modifiers} />;
}
