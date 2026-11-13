import { Row } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, paddingAll } from '@expo/ui/jetpack-compose/modifiers';

type ModifierConfig = ReturnType<typeof fillMaxWidth>;

export interface AppRowProps {
  children: React.ReactNode;
  align?: 'top' | 'center' | 'bottom';
  justify?: 'start' | 'end' | 'center' | 'spaceBetween' | 'spaceAround' | 'spaceEvenly';
  spacing?: number;
  fullWidth?: boolean;
  padding?: number;
}

export function AppRow({ children, align, justify, spacing, fullWidth, padding }: AppRowProps) {
  const modifiers: ModifierConfig[] = [];
  if (fullWidth) modifiers.push(fillMaxWidth());
  if (padding) modifiers.push(paddingAll(padding));
  return (
    <Row
      verticalAlignment={align}
      horizontalArrangement={spacing != null ? { spacedBy: spacing } : justify}
      modifiers={modifiers.length ? modifiers : undefined}
    >
      {children}
    </Row>
  );
}
