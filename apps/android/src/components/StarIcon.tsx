import React from 'react';
import Svg, { Polygon } from 'react-native-svg';

// 20x20 viewBox 中的五角星顶点（外径 9，内径 3.8，圆心 10,10）
const STAR_POINTS =
  '10,1 12.23,6.93 18.56,7.22 13.61,11.17 15.29,17.28 10,13.8 4.71,17.28 6.39,11.17 1.44,7.22 7.77,6.93';

interface StarIconProps {
  size: number;
  color: string;
  filled?: boolean;
}

export function StarIcon({ size, color, filled = true }: StarIconProps) {
  return (
    <Svg width={size} height={size} viewBox="0 0 20 20">
      <Polygon
        points={STAR_POINTS}
        fill={filled ? color : 'none'}
        stroke={color}
        strokeWidth={filled ? 1 : 1.5}
        strokeLinejoin="round"
      />
    </Svg>
  );
}
