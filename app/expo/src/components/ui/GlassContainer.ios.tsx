import React from 'react';
import { View, StyleSheet, useColorScheme, type ViewStyle, type StyleProp } from 'react-native';
import { GlassView, isLiquidGlassAvailable } from 'expo-glass-effect';
import { BlurView } from 'expo-blur';

export type GlassShape = 'circle' | 'capsule' | 'card';

export interface GlassContainerProps {
  shape: GlassShape;
  interactive?: boolean;
  cornerRadius?: number;
  style?: StyleProp<ViewStyle>;
  children: React.ReactNode;
}

const STROKE_OPACITY: Record<GlassShape, number> = {
  circle: 0.08,
  capsule: 0.06,
  card: 0.06,
};

const BLUR_TINT_LIGHT: Record<GlassShape, 'systemUltraThinMaterial' | 'regular'> = {
  circle: 'systemUltraThinMaterial',
  capsule: 'systemUltraThinMaterial',
  card: 'regular',
};

const BLUR_TINT_DARK: Record<GlassShape, 'systemUltraThinMaterialDark' | 'systemMaterialDark'> = {
  circle: 'systemUltraThinMaterialDark',
  capsule: 'systemUltraThinMaterialDark',
  card: 'systemMaterialDark',
};

function getShapeStyle(shape: GlassShape, cornerRadius?: number): ViewStyle {
  switch (shape) {
    case 'circle':
      return { borderRadius: 9999 };
    case 'capsule':
      return { borderRadius: 9999 };
    case 'card':
      return { borderRadius: cornerRadius ?? 18, borderCurve: 'continuous' as const };
  }
}

function getStrokeStyle(shape: GlassShape, cornerRadius?: number): ViewStyle {
  const base = getShapeStyle(shape, cornerRadius);
  return {
    ...StyleSheet.absoluteFill,
    ...base,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: `rgba(0,0,0,${STROKE_OPACITY[shape]})`,
  };
}

export function GlassContainer({
  shape,
  interactive = false,
  cornerRadius,
  style,
  children,
}: GlassContainerProps) {
  const colorScheme = useColorScheme();

  if (isLiquidGlassAvailable()) {
    return (
      <GlassView
        glassEffectStyle="regular"
        isInteractive={interactive}
        colorScheme="auto"
        style={[getShapeStyle(shape, cornerRadius), { overflow: 'hidden' }, style]}
      >
        {children}
      </GlassView>
    );
  }

  const isDark = colorScheme === 'dark';
  const tint = isDark ? BLUR_TINT_DARK[shape] : BLUR_TINT_LIGHT[shape];

  return (
    <View style={[getShapeStyle(shape, cornerRadius), { overflow: 'hidden' }, style]}>
      <BlurView intensity={80} tint={tint} style={StyleSheet.absoluteFill} />
      <View style={getStrokeStyle(shape, cornerRadius)} />
      {children}
    </View>
  );
}
