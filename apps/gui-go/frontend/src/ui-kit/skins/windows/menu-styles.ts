import { makeStyles, tokens } from '@fluentui/react-components'

// WinUI MenuFlyout: 8px corners, 40px-ish rows at standard density. The surface is solid: in the WebView2 run the
// translucent acrylic let the list text behind a flyout show through (this window has a solid material, so there
// is no backdrop worth blurring).
export const useMenuStyles = makeStyles({
  popover: {
    backgroundColor: tokens.colorNeutralBackground1,
    border: `${tokens.strokeWidthThin} solid var(--w11-stroke-elevation)`,
    borderRadius: tokens.borderRadiusXLarge,
    minWidth: '13rem',
  },
  // WinUI menu items use the primary text colour; Fluent web uses the secondary one.
  // Items are transparent over the flyout surface, as in MenuFlyout.
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
