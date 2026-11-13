/**
 * 设置列表行(Android):导航 / 动作 / 选择行共用的整行可点 ListItem。
 *
 * 点击挂在整个 ListItem 上(全行交互规范),尾部只是视觉提示:
 * - `chevron`:进入下一级页面;
 * - `{ action }`:行内动作的 primary 色标签(如「清理」「导出」);
 * - `{ value }`:当前取值 + 下拉箭头(SettingsSelectRow 用)。
 * 容器色取自 SettingsSectionItem 的 grouped 上下文,必须在行组件内部读取,否则 ListItem
 * 默认的 surface 色会盖住分组色块。
 */
import { memo } from 'react';
import {
  Box,
  Icon,
  ListItem,
  Row,
  Shape,
  Surface,
  Text as ComposeText,
  useMaterialColors,
} from '@expo/ui/jetpack-compose';
import {
  clickable,
  height as heightModifier,
  padding,
  testID as testIDModifier,
} from '@expo/ui/jetpack-compose/modifiers';

import { useTheme } from '@/hooks/useTheme';

import { useSettingsSectionRowColors } from '../SettingsSectionItem';
import { SettingsLeadingIcon, type SettingsLeadingIconTone } from './SettingsLeadingIcon';

const ICONS = {
  chevron: require('../../../assets/icons/chevron_right.xml'),
  expand: require('../../../assets/icons/expand_more.xml'),
};

const ACTION_STYLE = { typography: 'labelLarge' } as const;
const PILL_SHAPE = Shape.RoundedCorner({
  cornerRadii: { topStart: 20, topEnd: 20, bottomStart: 20, bottomEnd: 20 },
});

export type SettingsListRowTrailing =
  | 'chevron'
  | 'none'
  | { action: string; chevron?: boolean }
  | { pill: string }
  | { value: string };

/** `warning`:需要用户处理的状态行,整行换成 warningContainer 底色。 */
export type SettingsListRowTone = 'default' | 'warning';

export interface SettingsListRowProps {
  title: string;
  description?: string;
  /** 前导 40dp 圆形图标(SettingsLeadingIcon);二级页的普通设置行不带。 */
  icon?: number;
  iconTone?: SettingsLeadingIconTone;
  trailing?: SettingsListRowTrailing;
  onPress?: () => void;
  disabled?: boolean;
  /** 错误态:标题用 error 色(需处理的问题、破坏性操作)。 */
  destructive?: boolean;
  tone?: SettingsListRowTone;
  testID?: string;
}

export const SettingsListRow = memo(function SettingsListRow({
  title,
  description,
  icon,
  iconTone = 'primary',
  trailing = 'none',
  onPress,
  disabled = false,
  destructive = false,
  tone = 'default',
  testID,
}: SettingsListRowProps) {
  const colors = useMaterialColors();
  const sectionRowColors = useSettingsSectionRowColors();
  const { theme } = useTheme();
  const warning = tone === 'warning';
  const rowColors = warning
    ? {
        ...sectionRowColors,
        containerColor: theme.colors.warningContainer,
        contentColor: theme.colors.onWarningContainer,
        leadingContentColor: theme.colors.onWarningContainer,
        supportingContentColor: theme.colors.onWarningContainer,
      }
    : sectionRowColors;
  const interactive = onPress !== undefined && !disabled;
  const modifiers = [
    ...(testID ? [testIDModifier(testID)] : []),
    ...(interactive ? [clickable(onPress)] : []),
  ];
  const titleColor = disabled ? colors.onSurfaceVariant : destructive ? colors.error : undefined;

  return (
    <ListItem colors={rowColors} modifiers={modifiers}>
      {icon !== undefined ? (
        <ListItem.LeadingContent>
          {warning ? (
            <Icon source={icon} size={24} tint={theme.colors.onWarningContainer} />
          ) : (
            <SettingsLeadingIcon source={icon} tone={disabled ? 'muted' : iconTone} />
          )}
        </ListItem.LeadingContent>
      ) : null}
      <ListItem.HeadlineContent>
        <ComposeText color={titleColor}>{title}</ComposeText>
      </ListItem.HeadlineContent>
      {description ? (
        <ListItem.SupportingContent>
          <ComposeText>{description}</ComposeText>
        </ListItem.SupportingContent>
      ) : null}
      {trailing === 'chevron' ? (
        <ListItem.TrailingContent>
          <Icon source={ICONS.chevron} size={20} tint={colors.onSurfaceVariant} />
        </ListItem.TrailingContent>
      ) : typeof trailing === 'object' && 'action' in trailing ? (
        <ListItem.TrailingContent>
          <Row verticalAlignment="center">
            <ComposeText
              color={disabled ? colors.onSurfaceVariant : colors.primary}
              style={ACTION_STYLE}
            >
              {trailing.action}
            </ComposeText>
            {trailing.chevron ? (
              <Icon source={ICONS.chevron} size={20} tint={colors.onSurfaceVariant} />
            ) : null}
          </Row>
        </ListItem.TrailingContent>
      ) : typeof trailing === 'object' && 'pill' in trailing ? (
        <ListItem.TrailingContent>
          <Surface color={theme.colors.warning} shape={PILL_SHAPE}>
            <Box contentAlignment="center" modifiers={[heightModifier(40), padding(16, 0, 16, 0)]}>
              <ComposeText color={theme.colors.onWarning as string} style={ACTION_STYLE}>
                {trailing.pill}
              </ComposeText>
            </Box>
          </Surface>
        </ListItem.TrailingContent>
      ) : typeof trailing === 'object' ? (
        <ListItem.TrailingContent>
          <Row verticalAlignment="center">
            <ComposeText color={colors.onSurfaceVariant} style={ACTION_STYLE}>
              {trailing.value}
            </ComposeText>
            <Icon source={ICONS.expand} size={20} tint={colors.onSurfaceVariant} />
          </Row>
        </ListItem.TrailingContent>
      ) : null}
    </ListItem>
  );
});
