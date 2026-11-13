import React from 'react';
import { View, type ViewStyle, type StyleProp } from 'react-native';

export type GlassShape = 'circle' | 'capsule' | 'card';

export interface GlassContainerProps {
  shape: GlassShape;
  interactive?: boolean;
  cornerRadius?: number;
  style?: StyleProp<ViewStyle>;
  children: React.ReactNode;
}

export function GlassContainer({
  shape,
  interactive: _interactive,
  cornerRadius,
  style,
  children,
}: GlassContainerProps) {
  const shapeStyle: ViewStyle =
    shape === 'card' ? { borderRadius: cornerRadius ?? 18 } : { borderRadius: 9999 };

  return <View style={[shapeStyle, { overflow: 'hidden' }, style]}>{children}</View>;
}
