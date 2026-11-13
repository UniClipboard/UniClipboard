import { ListItem, Switch as ComposeSwitch, Text as ComposeText } from '@expo/ui/jetpack-compose';
import { testID as testIDModifier, toggleable } from '@expo/ui/jetpack-compose/modifiers';

import type { ReactNode } from 'react';

import { useSettingsSectionRowColors } from '../SettingsSectionItem';

interface SettingsSwitchRowProps {
  title: string;
  /** 可选前导图标(Compose Icon 节点) */
  leading?: ReactNode;
  testID?: string;
  description?: string;
  value: boolean;
  disabled?: boolean;
  onValueChange: (value: boolean) => void;
}

export function SettingsSwitchRow({
  title,
  leading,
  testID,
  description,
  value,
  disabled = false,
  onValueChange,
}: SettingsSwitchRowProps) {
  // grouped 分组内取色块容器色;card 分组下为 undefined,沿用 ListItem 默认
  const rowColors = useSettingsSectionRowColors();
  const toggle = () => {
    if (!disabled) onValueChange(!value);
  };
  return (
    <ListItem colors={rowColors} modifiers={[...(testID ? [testIDModifier(testID)] : []), toggleable(value, toggle, { role: 'switch' })]}>
      {leading ? <ListItem.LeadingContent>{leading}</ListItem.LeadingContent> : null}
      <ListItem.HeadlineContent>
        <ComposeText>{title}</ComposeText>
      </ListItem.HeadlineContent>
      {description ? (
        <ListItem.SupportingContent>
          <ComposeText>{description}</ComposeText>
        </ListItem.SupportingContent>
      ) : null}
      <ListItem.TrailingContent>
        <ComposeSwitch enabled={!disabled} value={value} onCheckedChange={toggle} />
      </ListItem.TrailingContent>
    </ListItem>
  );
}
