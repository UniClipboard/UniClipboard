import {
  makeStyles,
  tokens,
  Toolbar,
  ToolbarRadioButton,
  ToolbarRadioGroup,
} from '@fluentui/react-components'
import { createElement, useState } from 'react'
import type { SegmentedControlProps } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { WithTooltip } from './WithTooltip'

const useStyles = makeStyles({
  root: {
    display: 'inline-flex',
    padding: '0.125rem',
    gap: '0.125rem',
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: 'var(--w11-selected)',
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
  },
})

/**
 * WinUI has no segmented control; the Win11 board draws it as a radio group of toolbar buttons.
 * Fluent's toolbar supplies the single tab stop and arrow-key movement; the toolbar role itself is
 * removed so the named radio group is the only semantics, and selection follows arrow-key focus as
 * in a native radio group.
 */
export function SegmentedControl({
  items,
  value,
  defaultValue,
  onValueChange,
  label,
}: SegmentedControlProps) {
  const styles = useStyles()
  const [ownValue, setOwnValue] = useState(defaultValue)
  const current = value ?? ownValue
  const select = (next: string) => {
    if (next === current) return
    if (value === undefined) setOwnValue(next)
    onValueChange?.(next)
  }
  return (
    <Toolbar
      role="presentation"
      size="small"
      className={styles.root}
      checkedValues={{ segment: current === undefined ? [] : [current] }}
      onCheckedValueChange={(_, data) => {
        const next = data.checkedItems[0]
        if (next !== undefined) select(next)
      }}
    >
      <ToolbarRadioGroup aria-label={label}>
        {items.map(item => (
          <WithTooltip
            key={item.value}
            content={item.iconOnly ? item.label : undefined}
            relationship="inaccessible"
          >
            <ToolbarRadioButton
              name="segment"
              value={item.value}
              appearance="subtle"
              aria-label={item.iconOnly ? item.label : undefined}
              onFocus={event => {
                // Selection follows focus only when it moves between segments (arrow keys);
                // Tab entry and focus restoration come from outside the group and keep the value.
                const group = event.currentTarget.closest('[role="radiogroup"]')
                if (event.relatedTarget instanceof Node && group?.contains(event.relatedTarget))
                  select(item.value)
              }}
              icon={item.icon ? createElement(iconFor(item.icon)) : undefined}
            >
              {item.iconOnly ? undefined : item.label}
            </ToolbarRadioButton>
          </WithTooltip>
        ))}
      </ToolbarRadioGroup>
    </Toolbar>
  )
}
