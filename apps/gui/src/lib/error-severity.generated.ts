// @ts-nocheck
/* oxlint-disable */
// oxfmt-ignore
//
// Frozen error code table shared by the frontend and the desktop host (Go/Wails, apps/gui-go).
// It was generated from the error enums of the retired Tauri host; that generator no longer
// exists, so this file is maintained by hand. Keep it equal to the codes the host returns.
//
// 用户/校验类错误的 code 集合。前端中央 IPC 封装层据此判断某个 command
// 拒绝是否属于「用户操作错误」—— 是则不上报 Sentry(正常产品流程),
// 否则按系统错误上报。未列出的 code 默认按系统错误处理(fail-safe)。
//
// 权威来源:本文件(宿主错误码契约,手工维护)。

export const USER_FACING_ERROR_CODES: ReadonlySet<string> = new Set([
  "AccessibilityPermissionRequired",
  "Cancelled",
  "Conflict",
  "NotFound",
  "PROFILE_RECOVERY_REQUIRED",
  "SETUP_NOT_COMPLETED",
  "SPACE_NOT_INITIALIZED",
  "ValidationError",
  "WRONG_PASSPHRASE",
]);
