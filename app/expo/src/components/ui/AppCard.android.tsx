import { Card } from '@expo/ui/jetpack-compose';
import { fillMaxWidth } from '@expo/ui/jetpack-compose/modifiers';
import type { ColorValue } from 'react-native';

export interface AppCardProps {
  children: React.ReactNode;
  containerColor?: ColorValue;
  elevation?: number;
  fullWidth?: boolean;
}

export function AppCard({ children, containerColor, elevation, fullWidth }: AppCardProps) {
  return (
    <Card
      colors={containerColor ? { containerColor } : undefined}
      elevation={elevation}
      modifiers={fullWidth ? [fillMaxWidth()] : undefined}
    >
      {children}
    </Card>
  );
}
