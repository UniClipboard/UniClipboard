import {
  Button as SwiftUIButton,
  HStack,
  Image,
  Spacer,
  Text as SwiftUIText,
  VStack,
} from '@expo/ui/swift-ui';
import {
  accessibilityHint,
  accessibilityLabel,
  background,
  buttonStyle,
  contentShape,
  cornerRadius,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  shapes,
  truncationMode,
} from '@expo/ui/swift-ui/modifiers';
import { PlatformColor } from 'react-native';
import type { SFSymbol } from 'sf-symbols-typescript';
import { useTranslation } from 'react-i18next';

import type { EngineDevice } from '@/engine/types';
import { iosColors } from '@/theme/iosDesignTokens';
import { chevronColor, settingsTileColors, statusGreen } from '@/screens/settings/ios/common';

export const OFFLINE_DOT = '#C7C7CC';

export function deviceDisplayName(device: EngineDevice): string {
  return device.name || device.reported_name || device.id.slice(0, 8);
}

/** 设备页行的前导图标: 36pt 圆角方块，系统填充色底（对齐 v1 deviceRows.tsx） */
export function DeviceIconTile({ systemName }: { systemName: SFSymbol }) {
  return (
    <Image
      systemName={systemName}
      size={18}
      color={iosColors?.label}
      modifiers={[
        frame({ width: 36, height: 36 }),
        background(PlatformColor('tertiarySystemFill')),
        cornerRadius(9),
      ]}
    />
  );
}

export function engineDeviceStatus(
  device: EngineDevice,
  pair: (key: string) => string
): { label: string; dot: string } {
  if (device.trust === 'pending') {
    return { label: pair('review'), dot: settingsTileColors.orange };
  }
  const binding = device.bindings?.[0];
  if (!binding) {
    return { label: pair('noBinding'), dot: OFFLINE_DOT };
  }
  if (binding.connection === 'connected') {
    return binding.sync_enabled
      ? { label: pair('connected'), dot: statusGreen }
      : { label: pair('paused'), dot: settingsTileColors.orange };
  }
  if (binding.connection === 'connecting') {
    return { label: pair('connecting'), dot: settingsTileColors.blue };
  }
  return { label: pair('offline'), dot: OFFLINE_DOT };
}

export function LocalDeviceRow({
  name,
  deviceID,
  subtitle,
  onCopyID,
}: {
  name: string;
  deviceID: string;
  subtitle: string;
  onCopyID?: () => void;
}) {
  const row = (
    <HStack
      testID="devices-local-identity"
      spacing={16}
      alignment="center"
      modifiers={[frame({ maxWidth: Infinity, minHeight: 48 }), contentShape(shapes.rectangle())]}
    >
      <DeviceIconTile systemName="iphone" />
      <VStack alignment="leading" spacing={2}>
        <SwiftUIText>{name}</SwiftUIText>
        <SwiftUIText
          modifiers={[
            font({ size: 12, design: 'monospaced' }),
            foregroundStyle('secondary'),
            lineLimit(1),
            truncationMode('middle'),
          ]}
        >
          {subtitle || deviceID}
        </SwiftUIText>
      </VStack>
      <Spacer />
      {onCopyID ? <Image systemName="doc.on.doc" size={14} color={chevronColor} /> : null}
    </HStack>
  );

  if (!onCopyID) return row;
  return (
    <SwiftUIButton
      onPress={onCopyID}
      modifiers={[buttonStyle('plain'), accessibilityLabel(`${name}, ${deviceID}`)]}
    >
      {row}
    </SwiftUIButton>
  );
}

/** 设备行：整行可点（打开设备详情/审核），36pt 图标 + 名称/状态 + 8pt 状态点 + 箭头 */
export function EngineDeviceRow({
  testID,
  device,
  subtitle,
  statusLabel,
  dotColor,
  icon = 'laptopcomputer',
  onPress,
}: {
  testID?: string;
  device?: EngineDevice;
  subtitle?: string;
  statusLabel: string;
  dotColor: string | null;
  icon?: SFSymbol;
  onPress: () => void;
}) {
  const { t } = useTranslation('settingsSync');
  const title = device ? deviceDisplayName(device) : '';
  return (
    <SwiftUIButton
      testID={testID}
      onPress={onPress}
      modifiers={[
        buttonStyle('plain'),
        accessibilityLabel(`${title}, ${statusLabel}`),
        accessibilityHint(t('space.devices.manageHint')),
      ]}
    >
      <HStack
        spacing={16}
        alignment="center"
        modifiers={[frame({ maxWidth: Infinity, minHeight: 48 }), contentShape(shapes.rectangle())]}
      >
        <DeviceIconTile systemName={icon} />
        <VStack alignment="leading" spacing={1}>
          <SwiftUIText>{title}</SwiftUIText>
          <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
            {subtitle ?? statusLabel}
          </SwiftUIText>
        </VStack>
        <Spacer />
        {dotColor ? <Image systemName="circle.fill" size={8} color={dotColor} /> : null}
        <Image systemName="chevron.right" size={13} color={chevronColor} />
      </HStack>
    </SwiftUIButton>
  );
}
