import { Spacer } from '@expo/ui/jetpack-compose';
import {
  weight,
  height as heightModifier,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';

export interface AppSpacerProps {
  /** When set, the spacer takes a fixed size (dp) instead of flexing. */
  size?: number;
  /** Axis for the fixed size. @default 'vertical' */
  axis?: 'vertical' | 'horizontal';
}

export function AppSpacer({ size, axis = 'vertical' }: AppSpacerProps) {
  if (size != null) {
    return (
      <Spacer modifiers={[axis === 'horizontal' ? widthModifier(size) : heightModifier(size)]} />
    );
  }
  return <Spacer modifiers={[weight(1)]} />;
}
