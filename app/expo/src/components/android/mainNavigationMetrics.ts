/** M3 navigation rail 宽度(dp)。平板上主页面左侧被 rail 占去这部分宽度。 */
export const NAVIGATION_RAIL_WIDTH = 80;

/** 手机悬浮导航胶囊高度(dp):8 内边距 + 48 目的地 + 8 内边距,同 M3 floating toolbar。 */
export const FLOATING_NAV_HEIGHT = 64;

/** 悬浮导航胶囊与屏幕左缘 / 系统导航栏之间的间距(dp)。 */
export const FLOATING_NAV_MARGIN = 16;

/**
 * 悬浮导航胶囊在系统导航栏 inset 之上额外占去的底部高度(dp)。胶囊浮在内容之上,
 * 各目的地的滚动内容与 Snackbar 需在 inset 之外再让出这部分。
 */
export const FLOATING_NAV_CLEARANCE = FLOATING_NAV_HEIGHT + FLOATING_NAV_MARGIN;
