import React, { useState } from 'react';
import { DynamicColorIOS, PlatformColor } from 'react-native';
import {
  Button as SwiftUIButton,
  HStack,
  Image,
  Menu,
  Picker,
  Spacer,
  Text as SwiftUIText,
  Toggle,
  VStack,
} from '@expo/ui/swift-ui';
import {
  background,
  buttonStyle,
  contentShape,
  cornerRadius,
  disabled as disabledModifier,
  font,
  foregroundStyle,
  frame,
  glassEffect,
  listRowBackground,
  padding,
  opacity,
  pickerStyle,
  shapes,
  strokeBorder,
  tag,
  tint,
  accessibilityHint as accessibilityHintModifier,
  accessibilityLabel as accessibilityLabelModifier,
} from '@expo/ui/swift-ui/modifiers';
import type { SFSymbol } from 'sf-symbols-typescript';

import { iosAccentColor, iosColors } from '@/theme/iosDesignTokens';

/** iOS system palette for settings icon tiles (iOS Settings app style). */
export const settingsTileColors = {
  blue: '#007AFF',
  teal: '#32ADE6',
  green: '#34C759',
  orange: '#FF9500',
  red: '#FF3B30',
  indigo: '#5856D6',
  purple: '#AF52DE',
  pink: '#FF2D55',
  yellow: '#FFCC00',
  gray: '#8E8E93',
} as const;

export const chevronColor = '#8E8E93';
export const headerIconColor = '#AEAEB2';
export const statusGreen = settingsTileColors.green;
export const statusOrange = settingsTileColors.orange;

const settingsNavigationDelayMs = 120;
const settingsRowPressedColor = iosColors?.tertiarySystemFill ?? 'gray';

/**
 * iOS 系统绿开关。设置界面根 VStack 级联了墨色 accent tint(SettingsScreen.ios.tsx),
 * 会把 SwiftUI Toggle 的轨道也染成主题色;这里用 systemGreen 覆盖,让所有开关走 iOS
 * 原生绿轨道,而按钮/导航链接等仍保持 accent。新增设置开关统一用本组件而非裸 Toggle。
 */
const switchGreenTint = tint(PlatformColor('systemGreen'));

export function SettingsToggle({ modifiers, ...rest }: React.ComponentProps<typeof Toggle>) {
  return <Toggle {...rest} modifiers={[...(modifiers ?? []), switchGreenTint]} />;
}

/** Settings switch row with a colored icon tile and an optional description. */
export function IconToggleRow({
  testID,
  icon,
  iconColor,
  label,
  description,
  isOn,
  onIsOnChange,
}: {
  testID?: string;
  icon: SFSymbol;
  iconColor: string;
  label: string;
  description?: string;
  isOn: boolean;
  onIsOnChange: (v: boolean) => void;
}) {
  return (
    <HStack spacing={12} modifiers={[frame({ maxWidth: Infinity })]}>
      <SettingsIconTile systemName={icon} color={iconColor} />
      <SettingsToggle testID={testID} isOn={isOn} onIsOnChange={onIsOnChange}>
        <VStack alignment="leading" spacing={2}>
          <SwiftUIText>{label}</SwiftUIText>
          {description ? (
            <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
              {description}
            </SwiftUIText>
          ) : null}
        </VStack>
      </SettingsToggle>
    </HStack>
  );
}

/** Rounded-square colored icon, like the leading icons in the iOS Settings app. */
export function SettingsIconTile({ systemName, color }: { systemName: SFSymbol; color: string }) {
  return (
    <Image
      systemName={systemName}
      size={15}
      color="white"
      modifiers={[frame({ width: 28, height: 28 }), background(color), cornerRadius(6)]}
    />
  );
}

export interface SettingsNavRowProps {
  testID?: string;
  icon?: SFSymbol;
  iconColor?: string;
  title: string;
  subtitle?: string;
  /** Trailing secondary text (count, status, …). */
  value?: string;
  /** Hex color for the trailing text; defaults to secondary label. */
  valueColor?: string;
  badge?: string;
  destructive?: boolean;
  disabled?: boolean;
  selected?: boolean;
  showsChevron?: boolean;
  showsPressFeedback?: boolean;
  accessibilityHint?: string;
  /** Informational row: same layout, but not a control. */
  readOnly?: boolean;
  /** Visually de-emphasized without becoming a disabled control. */
  dimmed?: boolean;
  onPress?: () => void;
}

type SettingsRowContentProps = Pick<
  SettingsNavRowProps,
  | 'icon'
  | 'iconColor'
  | 'title'
  | 'subtitle'
  | 'value'
  | 'valueColor'
  | 'badge'
  | 'destructive'
  | 'selected'
  | 'showsChevron'
> & { dimmed?: boolean };

/**
 * Visual body shared by every full-width settings row. It fills the row and
 * defines a rectangular hit shape, so the empty trailing space is tappable.
 */
function SettingsRowContent({
  icon,
  iconColor,
  title,
  subtitle,
  value,
  valueColor,
  badge,
  destructive = false,
  selected = false,
  showsChevron = true,
  dimmed = false,
}: SettingsRowContentProps) {
  return (
    <HStack
      spacing={12}
      modifiers={[
        frame({ maxWidth: Infinity }),
        contentShape(shapes.rectangle()),
        ...(dimmed ? [opacity(0.5)] : []),
      ]}
    >
      {icon && iconColor ? (
        <SettingsIconTile systemName={icon} color={iconColor} />
      ) : icon ? (
        <Image
          systemName={icon}
          size={22}
          color={destructive ? PlatformColor('systemRed') : iosAccentColor}
          modifiers={[frame({ width: 28, height: 28 })]}
        />
      ) : null}
      <VStack alignment="leading" spacing={2}>
        <SwiftUIText
          modifiers={[foregroundStyle(destructive ? settingsTileColors.red : 'primary')]}
        >
          {title}
        </SwiftUIText>
        {subtitle ? (
          <SwiftUIText
            modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}
          >
            {subtitle}
          </SwiftUIText>
        ) : null}
      </VStack>
      {badge ? (
        <SwiftUIText
          modifiers={[
            font({ size: 11, weight: 'semibold' }),
            foregroundStyle('white'),
            padding({ horizontal: 6, vertical: 2 }),
            background(settingsTileColors.orange),
            cornerRadius(5),
          ]}
        >
          {badge}
        </SwiftUIText>
      ) : null}
      <Spacer />
      {value ? (
        <SwiftUIText
          modifiers={valueColor ? [foregroundStyle(valueColor)] : [foregroundStyle('secondary')]}
        >
          {value}
        </SwiftUIText>
      ) : null}
      {selected ? <Image systemName="checkmark" size={14} color={statusGreen} /> : null}
      {showsChevron ? <Image systemName="chevron.right" size={12} color={chevronColor} /> : null}
    </HStack>
  );
}

/** Full-width tappable row: optional icon tile + title … value + chevron. */
export function SettingsNavRow({
  testID,
  icon,
  iconColor,
  title,
  subtitle,
  value,
  valueColor,
  badge,
  destructive = false,
  disabled = false,
  selected = false,
  showsChevron = true,
  showsPressFeedback = true,
  accessibilityHint,
  readOnly = false,
  dimmed = false,
  onPress,
}: SettingsNavRowProps) {
  const [isPressed, setIsPressed] = useState(false);

  const handlePress = () => {
    if (disabled || isPressed || !onPress) return;

    if (!showsPressFeedback) {
      onPress();
      return;
    }

    setIsPressed(true);
    setTimeout(() => {
      setIsPressed(false);
      onPress();
    }, settingsNavigationDelayMs);
  };

  if (readOnly) {
    return (
      <SettingsRowContent
        icon={icon}
        iconColor={iconColor}
        title={title}
        subtitle={subtitle}
        value={value}
        valueColor={valueColor}
        badge={badge}
        showsChevron={false}
        dimmed={dimmed}
      />
    );
  }

  return (
    <SwiftUIButton
      testID={testID}
      role={destructive ? 'destructive' : undefined}
      onPress={handlePress}
      modifiers={[
        ...(isPressed ? [listRowBackground(settingsRowPressedColor)] : []),
        disabledModifier(disabled),
        opacity(disabled ? 0.35 : 1),
        ...(accessibilityHint ? [accessibilityHintModifier(accessibilityHint)] : []),
      ]}
    >
      <SettingsRowContent
        icon={icon}
        iconColor={iconColor}
        title={title}
        subtitle={subtitle}
        value={value}
        valueColor={valueColor}
        badge={badge}
        destructive={destructive}
        selected={selected}
        showsChevron={showsChevron}
      />
    </SwiftUIButton>
  );
}

/**
 * Full-width row that opens a native menu of choices instead of navigating.
 * Looks like SettingsNavRow; the whole row is the menu's trigger.
 */
export function SettingsMenuRow({
  testID,
  icon,
  iconColor,
  title,
  subtitle,
  children,
}: {
  testID?: string;
  icon?: SFSymbol;
  iconColor?: string;
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <Menu
      testID={testID}
      label={<SettingsRowContent icon={icon} iconColor={iconColor} title={title} subtitle={subtitle} />}
    >
      {children}
    </Menu>
  );
}

export interface SettingsPickerOption<T extends string> {
  value: T;
  label: string;
}

/**
 * Full-width menu picker row: icon tile + title … current value + ⌃⌄. The whole
 * row opens a native menu, so a single-choice setting needs no sub-page.
 */
export function SettingsPickerRow<T extends string>({
  testID,
  icon,
  iconColor,
  title,
  options,
  selection,
  onSelectionChange,
}: {
  testID?: string;
  icon: SFSymbol;
  iconColor: string;
  title: string;
  options: SettingsPickerOption<T>[];
  selection: T;
  onSelectionChange: (value: T) => void;
}) {
  return (
    <Picker
      testID={testID}
      selection={selection}
      onSelectionChange={(value) => onSelectionChange(value as T)}
      modifiers={[pickerStyle('menu')]}
      label={
        <HStack spacing={12}>
          <SettingsIconTile systemName={icon} color={iconColor} />
          <SwiftUIText>{title}</SwiftUIText>
        </HStack>
      }
    >
      {options.map((option) => (
        <SwiftUIText key={option.value} modifiers={[tag(option.value)]}>
          {option.label}
        </SwiftUIText>
      ))}
    </Picker>
  );
}

export type SetupStepState = 'done' | 'active' | 'pending';

// Darker than systemOrange / systemGreen so status text on white stays legible.
const setupStatusOrange = DynamicColorIOS({ light: '#C93400', dark: '#FF9F0A' });
const setupStatusGreen = DynamicColorIOS({ light: '#248A3D', dark: '#30D158' });
const setupActiveBlue = DynamicColorIOS({ light: '#0063CC', dark: '#409CFF' });
const setupActiveFill = DynamicColorIOS({ light: '#E5F1FF', dark: '#0B2A4A' });
const setupPendingRing = DynamicColorIOS({ light: '#C7C7CC', dark: '#545458' });

/**
 * Step number in a ring: blue with a light fill for the step to do now, gray
 * for later ones, and a filled green checkmark once done. Outlines go on the
 * container (see strokeBorder), never on a bare shape.
 */
function SetupStepBadge({ index, state }: { index: number; state: SetupStepState }) {
  if (state === 'done') {
    return (
      <Image
        systemName="checkmark"
        size={13}
        color="white"
        modifiers={[
          font({ weight: 'bold' }),
          frame({ width: 28, height: 28 }),
          background(setupStatusGreen, shapes.circle()),
        ]}
      />
    );
  }
  const active = state === 'active';
  return (
    <SwiftUIText
      modifiers={[
        font({ size: 15, weight: active ? 'bold' : 'semibold' }),
        foregroundStyle(active ? setupActiveBlue : 'secondary'),
        frame({ width: 28, height: 28 }),
        ...(active ? [background(setupActiveFill, shapes.circle())] : []),
        strokeBorder({
          content: active ? setupActiveBlue : setupPendingRing,
          style: { lineWidth: active ? 2 : 1.5 },
          shape: 'circle',
        }),
      ]}
    >
      {String(index)}
    </SwiftUIText>
  );
}

export interface SetupStepStatus {
  text: string;
  tone: 'ok' | 'warn' | 'muted';
}

/**
 * One step of a setup checklist: numbered badge (a green checkmark once done),
 * title, subtitle, the step's own live status on the trailing edge, and
 * optional detail content shown under the text while the step needs action.
 */
export function SetupStepRow({
  testID,
  index,
  state,
  title,
  subtitle,
  status,
  children,
}: {
  testID?: string;
  index: 1 | 2 | 3 | 4 | 5;
  state: SetupStepState;
  title: string;
  subtitle?: string;
  status?: SetupStepStatus;
  children?: React.ReactNode;
}) {
  const statusColor =
    status?.tone === 'ok'
      ? setupStatusGreen
      : status?.tone === 'warn'
        ? setupStatusOrange
        : undefined;
  return (
    <HStack
      testID={testID}
      spacing={14}
      alignment="top"
      modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' }), padding({ vertical: 4 })]}
    >
      <SetupStepBadge index={index} state={state} />
      <VStack
        alignment="leading"
        spacing={2}
        modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' })]}
      >
        <HStack alignment="firstTextBaseline" spacing={8}>
          <SwiftUIText
            modifiers={[
              font({ weight: state === 'active' ? 'semibold' : 'regular' }),
              foregroundStyle(state === 'pending' ? 'secondary' : 'primary'),
            ]}
          >
            {title}
          </SwiftUIText>
          <Spacer />
          {status ? (
            <SwiftUIText
              modifiers={[
                font({ size: 15 }),
                foregroundStyle(statusColor ?? 'secondary'),
              ]}
            >
              {status.text}
            </SwiftUIText>
          ) : null}
        </HStack>
        {subtitle ? (
          <SwiftUIText modifiers={[font({ size: 13 }), foregroundStyle('secondary')]}>
            {subtitle}
          </SwiftUIText>
        ) : null}
        {children ? (
          <VStack
            alignment="leading"
            spacing={10}
            modifiers={[frame({ maxWidth: Infinity, alignment: 'leading' }), padding({ top: 8 })]}
          >
            {children}
          </VStack>
        ) : null}
      </VStack>
    </HStack>
  );
}

/** Glass circular header button (back chevron, add, …) matching the sheet header style. */
export function HeaderCircleButton({
  testID,
  systemName,
  onPress,
  accessibilityLabel,
  disabled = false,
}: {
  testID?: string;
  systemName: SFSymbol;
  onPress: () => void;
  accessibilityLabel?: string;
  disabled?: boolean;
}) {
  return (
    <SwiftUIButton
      testID={testID}
      onPress={onPress}
      modifiers={[
        buttonStyle('plain'),
        disabledModifier(disabled),
        glassEffect({ glass: { variant: 'regular', interactive: true }, shape: 'circle' }),
        ...(accessibilityLabel ? [accessibilityLabelModifier(accessibilityLabel)] : []),
      ]}
    >
      <Image
        systemName={systemName}
        size={20}
        color={headerIconColor}
        modifiers={[font({ weight: 'semibold' }), padding()]}
      />
    </SwiftUIButton>
  );
}
