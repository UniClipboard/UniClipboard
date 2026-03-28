---
status: resolved
trigger: '在 unlock（解锁）之后，没有正常进入 dashboard 页面，而是黑/白屏，需要 reload 之后才能正常进入'
created: 2026-03-27T00:00:00Z
updated: 2026-03-27T01:15:00Z
commit: 7f76659b
---

## Current Focus

hypothesis: 确认根本原因有两个协同作用的缺陷：

1. 主要原因：`useGetEncryptionSessionStatusQuery` 使用 `skip: isSetupActive`，当 `isSetupActive` 从 true→false（hydrated 初始化完成）时，RTK Query 开始新查询，`encryptionLoading=true`，`AppContent` 返回 null（黑屏）。此时即使 SessionReady 事件到来，`encryptionLoading=true` 使得渲染始终为 null，直到 RTK Query 查询完成。
2. 次要原因：`handleUnlock` 不检查 `unlockEncryptionSession()` 的 boolean 返回值，当后端返回 false 时（例如已就绪），依然 setIsExiting(true)，触发淡出动画，但 SessionReady 不会到来，导致 UnlockPage 淡出后黑屏。
   fix: 修复 `AppContent` 中 `encryptionLoading` 的判断逻辑，让它在 `encryptionStatus` 已知（非 null）时不返回 null；同时修复 handleUnlock 检查返回值。
   test: 已完成代码分析
   expecting: 修复后解锁后直接进入 Dashboard
   next_action: 实施修复

## Symptoms

expected: 用户在锁屏界面输入密码解锁后，应自动导航到 dashboard 页面并正常渲染内容
actual: 解锁后页面变成黑屏或白屏，没有内容显示。手动 reload（刷新）后可以正常进入 dashboard
errors: 未确认，需要调查
reproduction: 每次 unlock 后都能 100% 复现
started: 持续存在的问题

## Eliminated

- hypothesis: BrowserRouter 路由状态不匹配导致 Routes 渲染空白
  evidence: Routes 始终在 BrowserRouter 内部，location 始终是 `/`，路由正确匹配 DashboardPage
  timestamp: 2026-03-27T00:15:00Z

- hypothesis: RTK Query 重新查询覆盖了 encryptionStatus（useEffect([encryptionData]) 覆盖 SessionReady 更新）
  evidence: 在手动解锁场景中，encryptionData 不会改变（RTK Query 不会重新查询），useEffect([encryptionData]) 不会再次触发，不会覆盖
  timestamp: 2026-03-27T00:20:00Z

- hypothesis: SessionReady 事件序列化格式不匹配
  evidence: 后端发出 `{ "type": "SessionReady" }`，前端处理正确（`event.payload?.type === "SessionReady"`）
  timestamp: 2026-03-27T00:25:00Z

- hypothesis: DashboardPage 渲染出错导致黑屏
  evidence: ClipboardContent 有完整的空状态 UI，不会返回 null；Sentry ErrorBoundary 会捕获渲染错误并显示 "Something went wrong"，不是黑屏
  timestamp: 2026-03-27T00:30:00Z

## Evidence

- timestamp: 2026-03-27T00:05:00Z
  checked: src/pages/UnlockPage.tsx
  found: handleUnlock 调用 unlockEncryptionSession()，成功后只设置 isExiting=true（触发退出动画），注释中说 "The App component will handle the navigation when the session becomes ready"。UnlockPage 自身不做路由导航。
  implication: 导航逻辑全部依赖 App.tsx 中监听 encryption://event 的 SessionReady 事件来更新状态

- timestamp: 2026-03-27T00:06:00Z
  checked: src/App.tsx - AppContent 组件
  found: AppContent 维护本地 encryptionStatus state。通过 useGetEncryptionSessionStatusQuery（RTK Query，一次性查询）初始化，通过 listen('encryption://event') 监听 SessionReady 更新。当 session_ready=true 时渲染 Routes，否则渲染 UnlockPage。
  implication: 整个 UnlockPage → Routes 的切换是靠 AppContent 的条件渲染实现的，不是路由导航。

- timestamp: 2026-03-27T00:07:00Z
  checked: src/App.tsx - AppContent 条件渲染逻辑（156-188行）
  found: |

  ```
  if (resolvedEncryptionStatus?.initialized && !resolvedEncryptionStatus?.session_ready) {
    return <UnlockPage /> + <PairingNotificationProvider />
  }
  return <ShortcutProvider><GlobalShortcuts /><Routes>...</Routes></ShortcutProvider>
  ```

  关键问题：AppContent 在 UnlockPage 状态时，完全没有挂载 Routes 和 ShortcutProvider。
  当 SessionReady 触发后，Routes 是全新挂载的（从未渲染过）。
  implication: 路由组件从零开始初始化，BrowserRouter 的当前 location 将决定 Routes 渲染哪个路由

- timestamp: 2026-03-27T00:08:00Z
  checked: src/App.tsx - 路由结构（168-184行）
  found: |
  Routes 定义:
  - / -> DashboardPage (通过 AuthenticatedLayout 包装)
  - /devices -> DevicesPage
  - /settings -> SettingsPage
  - - -> <Navigate to="/" replace />

  AppContent 被包在 BrowserRouter 中，BrowserRouter 的初始路径是 "/"。
  UnlockPage 期间没有任何路由切换（UnlockPage 直接 return，不在 Routes 内）。
  所以 SessionReady 后，BrowserRouter location 仍应是 "/"，Routes 应匹配 /，渲染 DashboardPage。
  implication: 路由本身理论上应该能匹配，但问题可能在于其他地方

- timestamp: 2026-03-27T00:09:00Z
  checked: src/store/api.ts - useGetEncryptionSessionStatusQuery
  found: RTK Query 使用 fakeBaseQuery，一次性查询 getEncryptionSessionStatus。没有自动重新验证（polling 或 invalidation）机制。TagType 是 EncryptionStatus 但没有任何 mutation 会 invalidate 这个 tag。
  implication: 解锁后 encryptionStatus 的更新完全依赖 encryption://event 的 SessionReady 事件监听，而非 RTK Query 重新获取

- timestamp: 2026-03-27T00:10:00Z
  checked: src/App.tsx - encryptionStatus 更新逻辑（95-119行）
  found: |
  有两个来源更新 encryptionStatus：
  1. useGetEncryptionSessionStatusQuery 初次查询结果（通过 useEffect 同步到 encryptionStatus state）
  2. listen('encryption://event') 监听 SessionReady，只更新 session_ready=true（不更新 initialized）

  问题发现！SessionReady 监听的处理（101-104行）：

  ```
  setEncryptionStatus(prev =>
    prev ? { ...prev, session_ready: true } : { initialized: true, session_ready: true }
  )
  ```

  如果 prev 为 null（RTK Query 查询还没完成），则设置 { initialized: true, session_ready: true }。
  但如果 RTK Query 查询已完成，prev 已经是 { initialized: true, session_ready: false }，
  则设置为 { initialized: true, session_ready: true }。

  这看起来逻辑正确，但...
  implication: 需要进一步检查 resolvedEncryptionStatus 的计算方式

- timestamp: 2026-03-27T00:11:00Z
  checked: src/App.tsx 第133行 resolvedEncryptionStatus 计算
  found: |

  ```
  const resolvedEncryptionStatus = encryptionStatus ?? encryptionData ?? null
  ```

  encryptionStatus 是本地 useState（初始 null）
  encryptionData 是 RTK Query 返回值

  当 SessionReady 事件触发时，encryptionStatus 被 setEncryptionStatus 更新为 {initialized:true, session_ready:true}
  所以 resolvedEncryptionStatus 变为 {initialized:true, session_ready:true}
  条件 initialized && !session_ready 变为 false
  AppContent 不再渲染 UnlockPage，而是渲染 Routes 分支
  implication: 状态切换逻辑正确

- timestamp: 2026-03-27T00:35:00Z
  checked: src/App.tsx useGetEncryptionSessionStatusQuery skip 参数行为
  found: |
  `useGetEncryptionSessionStatusQuery(undefined, { skip: isSetupActive })`

  isSetupActive = isSetupGateActive(setupState, hydrated, showCompletionStep)
  = !hydrated || setupState !== 'Completed' || showCompletionStep

  当 hydrated=false（初始状态）时，isSetupActive=true → skip=true → encryptionLoading=false, encryptionData=undefined
  当 hydrated 变为 true（setupRealtimeStore 初始化完成后）时，isSetupActive=false → skip=false
  → RTK Query 开始新查询 → encryptionLoading=true → AppContent return null → 黑屏！

  黑屏持续时间 = getEncryptionSessionStatus() Tauri 命令的执行时间

  这个黑屏在每次 App 启动时都会发生（短暂），但如果恰好在解锁操作完成时触发，
  就会导致解锁后黑屏（因为 encryptionLoading=true 会屏蔽 encryptionStatus=true 的渲染）
  implication: 这是导致黑屏的核心机制

- timestamp: 2026-03-27T00:40:00Z
  checked: src/pages/UnlockPage.tsx handleUnlock 函数
  found: |

  ```tsx
  const handleUnlock = async () => {
    setUnlocking(true)
    setIsExiting(false)
    try {
      await unlockEncryptionSession() // 返回 Promise<boolean>
      // 注释说"App component will handle the navigation"
      setIsExiting(true) // 无论返回 true 还是 false，都设置！
    } catch (error) {
      // 只有抛出异常才重置
      setUnlocking(false)
      setIsExiting(false)
    }
  }
  ```

  unlock_encryption_session_with_runtime 的返回值：
  - Ok(true)：解锁成功，ensure_ready() 执行，SessionReady 事件发出
  - Ok(false)：加密未初始化，跳过解锁（不发 SessionReady）
  - Err：解锁失败，发出 SessionFailed 事件（前端会 catch 异常）

  如果返回 Ok(false)：前端 `await unlockEncryptionSession()` 返回 false（不抛异常），
  但 handleUnlock 还是调用 setIsExiting(true)，触发淡出动画。
  SessionReady 不会到来，encryptionStatus.session_ready 仍然 false。
  UnlockPage 淡出（opacity-0）后，背景（bg-background=黑/白）显露 → 黑/白屏！

  但这个场景（初始化但未 session_ready 的情况下 auto_unlock 返回 false）比较罕见。
  implication: handleUnlock 需要检查返回值，如果返回 false 应该不触发退出动画或显示错误

## Resolution

root_cause: |
两个协同作用的缺陷导致解锁后黑/白屏：

**缺陷 1（主要）：RTK Query `encryptionLoading=true` 屏蔽了解锁后的渲染**

`AppContent` 在 `encryptionLoading=true` 时返回 null（黑屏）。
当 `isSetupActive` 从 true→false（即 `hydrated` 初始化完成），RTK Query 的 `skip` 从 true→false，
触发新查询，`isLoading=true`。此期间即使 SessionReady 事件到来（设置 `session_ready=true`），
渲染仍然是 null（黑屏）。

**缺陷 2（次要，放大缺陷1的影响）：RTK Query 完成后覆盖了 SessionReady 设置的状态**

`useEffect([encryptionData])` 会将 RTK Query 的查询结果直接覆盖 `encryptionStatus`。
如果 RTK Query 查询在 unlock 完成之前就返回（返回 `{ session_ready: false }`，因为此时后端还没
unlock），那么会将 `encryptionStatus` 从 `{ session_ready: true }`（由 SessionReady 事件设置）
覆盖回 `{ session_ready: false }`，导致重新显示 UnlockPage。

由于 UnlockPage 此时 `isExiting=true`（淡出动画），内容完全透明，背景色显露，形成黑/白屏。

**缺陷 3（独立问题）：handleUnlock 不检查 unlockEncryptionSession() 的返回值**

`unlock_encryption_session` 返回 `Ok(false)` 表示加密未初始化（不发 SessionReady）。
但 `handleUnlock` 只检查是否抛出异常，返回 false 时仍然 `setIsExiting(true)`，
触发 UnlockPage 淡出但 dashboard 不出现，背景显露 → 黑屏。

fix: |

1. `src/App.tsx`：修改 encryptionLoading 判断，只在 `encryptionStatus===null` 时才因 loading 返回 null
   `if (encryptionLoading && encryptionStatus === null) { return null }`

2. `src/App.tsx`：修改 `useEffect([encryptionData])` 中对 encryptionStatus 的更新，防止降级：
   如果当前 `session_ready=true`，不允许被 `session_ready=false` 的 encryptionData 覆盖。

3. `src/pages/UnlockPage.tsx`：handleUnlock 检查返回值，只有 `unlocked===true` 时才 setIsExiting(true)；
   返回 false 时重置 setUnlocking(false)。

verification: 代码分析确认修复逻辑正确，TypeScript 类型检查无新错误，等待用户验证
files_changed:

- src/App.tsx
- src/pages/UnlockPage.tsx
