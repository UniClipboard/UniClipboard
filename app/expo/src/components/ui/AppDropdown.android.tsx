import { useState } from 'react';
import {
  ExposedDropdownMenuBox,
  ExposedDropdownMenu,
  DropdownMenuItem,
  Icon,
  OutlinedButton,
  Spacer,
  Text as ComposeText,
} from '@expo/ui/jetpack-compose';
import {
  menuAnchor,
  fillMaxWidth,
  weight,
  width as widthModifier,
} from '@expo/ui/jetpack-compose/modifiers';

const ICONS = {
  expandMore: require('../../assets/icons/expand_more.xml'),
};

export interface AppDropdownOption<T extends string = string> {
  label: string;
  value: T;
}

export interface AppDropdownProps<T extends string = string> {
  options: AppDropdownOption<T>[];
  selectedValue?: T;
  onSelect: (value: T) => void;
  placeholder?: string;
  label?: string;
  disabled?: boolean;
  fullWidth?: boolean;
  width?: number;
}

export function AppDropdown<T extends string = string>({
  options,
  selectedValue,
  onSelect,
  placeholder,
  label,
  disabled,
  fullWidth,
  width,
}: AppDropdownProps<T>) {
  const [expanded, setExpanded] = useState(false);
  const selectedLabel = options.find((o) => o.value === selectedValue)?.label ?? placeholder ?? '';

  const boxModifiers =
    width !== undefined ? [widthModifier(width)] : fullWidth ? [fillMaxWidth()] : undefined;
  const buttonModifiers =
    width !== undefined || fullWidth ? [menuAnchor(), fillMaxWidth()] : [menuAnchor()];

  return (
    <ExposedDropdownMenuBox
      expanded={expanded}
      onExpandedChange={(next) => {
        if (!disabled) setExpanded(next);
      }}
      modifiers={boxModifiers}
    >
      <OutlinedButton onClick={undefined} enabled={!disabled} modifiers={buttonModifiers}>
        <ComposeText>{label ? `${label}: ${selectedLabel}` : selectedLabel}</ComposeText>
        <Spacer modifiers={[weight(1)]} />
        <Icon source={ICONS.expandMore} size={18} />
      </OutlinedButton>
      <ExposedDropdownMenu expanded={expanded} onDismissRequest={() => setExpanded(false)}>
        {options.map((option) => (
          <DropdownMenuItem
            key={option.value}
            onClick={() => {
              onSelect(option.value);
              setExpanded(false);
            }}
          >
            <DropdownMenuItem.Text>
              <ComposeText>{option.label}</ComposeText>
            </DropdownMenuItem.Text>
          </DropdownMenuItem>
        ))}
      </ExposedDropdownMenu>
    </ExposedDropdownMenuBox>
  );
}
