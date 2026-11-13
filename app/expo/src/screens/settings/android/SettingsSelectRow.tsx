/**
 * 单选设置行(Android):整行作为 DropdownMenu 的触发区,尾部显示当前取值。
 *
 * 取代行尾独立的下拉按钮——点行内任意位置(含空白处)都会展开菜单。
 * Compose DropdownMenu 从锚点的起始边展开;若把整行当锚点,菜单会贴在最左侧。
 * 因此整行只负责点击,菜单锚点是行底部末端的零尺寸 Box,菜单右对齐到当前取值下方。
 */
import { useState } from 'react';
import {
  Box,
  DropdownMenu,
  DropdownMenuItem,
  Icon,
  Text as ComposeText,
} from '@expo/ui/jetpack-compose';
import { fillMaxWidth } from '@expo/ui/jetpack-compose/modifiers';

import { SettingsListRow } from './SettingsListRow';

const CHECK_ICON = require('../../../assets/icons/check.xml');

export interface SettingsSelectOption<T extends string> {
  label: string;
  value: T;
}

interface SettingsSelectRowProps<T extends string> {
  title: string;
  description?: string;
  /** 前导图标(一级页中枢行用),透传给 SettingsListRow。 */
  icon?: number;
  options: SettingsSelectOption<T>[];
  selectedValue: T | undefined;
  onSelect: (value: T) => void;
  disabled?: boolean;
  testID?: string;
}

export function SettingsSelectRow<T extends string>({
  title,
  description,
  icon,
  options,
  selectedValue,
  onSelect,
  disabled,
  testID,
}: SettingsSelectRowProps<T>) {
  const [expanded, setExpanded] = useState(false);
  const selectedLabel = options.find((option) => option.value === selectedValue)?.label ?? '';

  return (
    <Box contentAlignment="bottomEnd" modifiers={[fillMaxWidth()]}>
      <SettingsListRow
        title={title}
        description={description}
        icon={icon}
        trailing={{ value: selectedLabel }}
        onPress={() => setExpanded(true)}
        disabled={disabled}
        testID={testID}
      />
      <DropdownMenu expanded={expanded} onDismissRequest={() => setExpanded(false)}>
        <DropdownMenu.Items>
          {options.map((option) => (
            <DropdownMenuItem
              key={option.value}
              onClick={() => {
                setExpanded(false);
                if (option.value !== selectedValue) onSelect(option.value);
              }}
            >
              <DropdownMenuItem.Text>
                <ComposeText>{option.label}</ComposeText>
              </DropdownMenuItem.Text>
              {option.value === selectedValue ? (
                <DropdownMenuItem.TrailingIcon>
                  <Icon source={CHECK_ICON} size={20} />
                </DropdownMenuItem.TrailingIcon>
              ) : null}
            </DropdownMenuItem>
          ))}
        </DropdownMenu.Items>
      </DropdownMenu>
    </Box>
  );
}
