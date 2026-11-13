import { Column } from '@expo/ui/jetpack-compose';
import { fillMaxWidth, paddingAll } from '@expo/ui/jetpack-compose/modifiers';

type ModifierConfig = ReturnType<typeof fillMaxWidth>;

export interface AppColumnProps {
  children: React.ReactNode;
  align?: 'start' | 'center' | 'end';
  justify?: 'top' | 'bottom' | 'center' | 'spaceBetween' | 'spaceAround' | 'spaceEvenly';
  spacing?: number;
  fullWidth?: boolean;
  padding?: number;
}

export function AppColumn({
  children,
  align,
  justify,
  spacing,
  fullWidth,
  padding,
}: AppColumnProps) {
  const modifiers: ModifierConfig[] = [];
  if (fullWidth) modifiers.push(fillMaxWidth());
  if (padding) modifiers.push(paddingAll(padding));
  return (
    <Column
      horizontalAlignment={align}
      verticalArrangement={spacing != null ? { spacedBy: spacing } : justify}
      modifiers={modifiers.length ? modifiers : undefined}
    >
      {children}
    </Column>
  );
}
