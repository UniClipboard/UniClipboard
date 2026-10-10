/**
 * Auto-inline preview threshold (D6 / ADR-008 P3-d).
 *
 * Images at or below this size inline directly via the daemon blob endpoint
 * (`GET /clipboard/blobs/:id`). Larger originals are NOT auto-pulled — the
 * preview surfaces a placeholder and the user must explicitly request the full
 * image. This keeps large originals off the auto-load path (transport cost +
 * daemon resident-set growth from full-buffer pulls; see
 * `docs/architecture/adr-008-perf-spike-results.md` §4, where 8 MiB is the
 * ruled-in inline threshold).
 *
 * 自动内联预览阈值（D6）：≤8MiB 的图片走 blob 端点直接内联；>8MiB 的原图不自动
 * 拉取，预览面显示占位并由用户显式发起加载。
 */
export const INLINE_PREVIEW_MAX_BYTES = 8 * 1024 * 1024
