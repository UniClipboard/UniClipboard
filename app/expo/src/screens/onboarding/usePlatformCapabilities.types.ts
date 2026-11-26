import type { ReactNode } from 'react';

export type CapabilityIconKind = 'paste' | 'keyboard' | 'share' | 'bell' | 'battery';
export type CapabilityButtonTone = 'primary' | 'soft' | 'ok' | 'warn';

export interface CapabilityRowItem {
  id: string;
  icon: CapabilityIconKind;
  iosTileColor: string;
  title: string;
  subtitle: string;
  actionLabel: string;
  actionTone: CapabilityButtonTone;
  onPress: () => void;
  testID: string;
}

export interface ReadySummaryItem {
  id: string;
  icon: CapabilityIconKind;
  iosTileColor: string;
  title: string;
  statusLabel: string;
  isPositive: boolean;
}

export interface PlatformCapabilitiesController {
  stepLabel: string;
  title: string;
  subtitle: string;
  rows: CapabilityRowItem[];
  readyItems: ReadySummaryItem[];
  renderOverlays: () => ReactNode;
}
