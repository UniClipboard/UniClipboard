/**
 * 主题颜色 —— 平台解析入口(fallback)
 *
 * 本文件是 Metro 平台文件解析的默认/兜底实现:
 *   - Android → colors.android.ts(M3 十六进制)
 *   - iOS     → colors.ios.ts(PlatformColor / DynamicColorIOS)
 *   - 其它(jest / tsc / web)→ 本文件,复用 Android 色板
 *
 * 调用点统一 `import { ... } from '@/theme'`,不必关心平台;需要颜色契约类型时用 ColorScheme。
 * 新增/修改颜色:改 ColorScheme 契约(colors.types.ts)+ 两端实现,不要在这里加平台分支。
 */

export * from './colors.android';
