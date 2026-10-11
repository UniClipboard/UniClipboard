import { makeStyles, tokens } from '@fluentui/react-components'

// WinUI MenuFlyout: acrylic surface, 8px corners, 40px-ish rows at standard density.
export const useMenuStyles = makeStyles({
  popover: {
    backgroundColor: 'var(--w11-acrylic)',
    backdropFilter: 'blur(30px) saturate(125%)',
    borderRadius: tokens.borderRadiusXLarge,
    minWidth: '13rem',
  },
  // WinUI menu items use the primary text colour; Fluent web uses the secondary one.
  // Items are transparent so the acrylic surface shows through, as in MenuFlyout.
  item: {
    color: tokens.colorNeutralForeground1,
    backgroundColor: 'transparent',
    ':hover': {
      backgroundColor: tokens.colorSubtleBackgroundHover,
      color: tokens.colorNeutralForeground1,
    },
    ':hover:active': { backgroundColor: tokens.colorSubtleBackgroundPressed },
  },
  destructive: {
    color: tokens.colorPaletteRedForeground1,
    ':hover': { color: tokens.colorPaletteRedForeground1 },
  },
  swatch: {
    display: 'inline-block',
    width: '0.75rem',
    height: '0.75rem',
    borderRadius: '0.1875rem',
  },
})
