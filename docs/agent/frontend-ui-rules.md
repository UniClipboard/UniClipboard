# Frontend and UI Rules

Use this document when editing React, TypeScript, Tailwind, UX flows, or frontend-facing daemon/Tauri integration.

## Frontend Layout Rules

- **No fixed-pixel layouts.**
  - Use **Tailwind utilities** or **rem** units.

## Theme Support Best Practices

Always verify components in both light and dark themes.

### Container Components

- Use `bg-card` + `text-card-foreground` for containers with content.
- Use `bg-background` only for page/base backgrounds.
- Use `bg-muted` for disabled/readonly states with `text-foreground`.

Examples:

```tsx
// ❌ Wrong
<DialogContent className="bg-background" />

// ✅ Correct
<DialogContent className="bg-card text-card-foreground" />

// ❌ Wrong
<input className="bg-muted text-muted-foreground" readOnly />

// ✅ Correct
<input className="bg-muted/50 text-foreground" readOnly />
```

### Status Messages

- Add `border border-{color}/20` to banners for better light-mode visibility.
- Use `font-medium` on status text when readability matters.
- Prefer `/70` over `/60` hover opacity when contrast is marginal.

## Frontend Architecture Notes

- Prefer API wrappers in `src/api/*` and shared helpers over direct `invoke()` in components.
- Keep route gating in `App.tsx` or layout-level logic, not duplicated in leaf components.
- Avoid parallel state sources for the same domain (local cache + Redux for the same truth).
- Match TypeScript DTO field names to actual Rust serde output. Do not assume global snake_case or camelCase consistency.

## Calling Tauri commands (issue #698)

All `#[tauri::command]` definitions are exported as a typed `commands` object via
`tauri-specta`. Frontend code MUST go through the wrapper in `src/lib/ipc.ts`
rather than calling `invoke()` / `invokeWithTrace()` with a stringly-typed
command name.

```ts
// ❌ Wrong — stringly-typed, no compile-time safety
await invokeWithTrace('update_mobile_sync_settings', patch)

// ✅ Correct — typed, fail-build on Rust signature drift
import { commands } from '@/lib/ipc'
await commands.updateMobileSyncSettings(patch)
```

The wrapper preserves trace_id injection, Sentry breadcrumbs, and arg
redaction. The generated bindings live in `src/lib/ipc-bindings.generated.ts`
(git-tracked, do not hand-edit). When you change a Rust command/DTO, regenerate
with `cargo test -p uc-tauri --test specta_export` and commit the diff — see
`docs/agent/rust-tauri-rules.md` ("tauri-specta IPC bindings") for the Rust
side of the contract.

## Test Execution Note

For frontend unit tests involving Vitest mocks, fake timers, or jsdom, prefer `npx vitest run` over `bun test`.

## React Doctor 规范（写代码时主动避免）

`bun run doctor`(`npx react-doctor@latest`) 是项目的健康度评分工具，任何 PR 都要避免引入新规则。下列是历史踩坑总结，新代码必须遵守;触发即视为回归。

### React 19 API 使用

- **不要用 `forwardRef`**。React 19 把 `ref` 当普通 prop。新组件直接 `function C({ ref, ...props }: Props & { ref?: React.Ref<T> })`,不要包 `forwardRef`。
- **不要用 `useContext`**,改用 `use(Context)`。`use()` 可条件调用，适用范围更广。
- **不要用 `flushSync`** 触发非紧急更新，用 `startTransition`。`document.startViewTransition()` 与 React 的 `<ViewTransition>` 不兼容 (react-doctor 会标 `no-document-start-view-transition`),除非有特殊需求 (如 `src/lib/theme-transition.ts` 的 circular reveal) 否则避免。
- **不要用 `React.MutableRefObject`**(已废弃),用 `React.RefObject<T | null>`。

### useEffect 卫生

- **必须返回 cleanup**:`useEffect` 里凡是 `setTimeout` / `setInterval` / `addEventListener` / `subscribe` / `listen` / `on` / `watch`,return 一个清理函数。把 timer id / unsubscribe 放在同一个 effect 的闭包里，**不要拆到另一个 unmount-only effect**,react-doctor 不识别那种拆分。
- **`location` / `window` / `document` 这类全局 mutable 不要放进 deps**。来自 `useLocation()` 的 `location` 同样会被 react-doctor 误报，**给它起别名** `routerLocation` 或解构出来再用。
- **prop 回调读但不影响订阅的，用 `useEffectEvent` 包**:`const onCancelEvent = useEffectEvent(onCancel)`,然后从 deps 移除。这能避免 effect 因函数引用变化反复 re-sync。
- **不要在 `useEffect` 里 `setState` 初始化**(`useState(undefined)` + `useEffect(() => setState(initial), [])`),改成 `useState(() => initial)` 或 callback ref(DOM 测量场景)。
- **不要在 deps 里写 `ref.current`**,而是在 effect body 内读;真要响应外部变化，用 `useSyncExternalStore`。

### State 设计反模式

下面这些 react-doctor 规则一旦命中通常意味着架构有问题，要在设计阶段避免，不要写出来再修：

- **`no-derived-state`**:不要 `useState(prop)` 然后 `useEffect(() => setState(prop), [prop])`。能在 render 里算出来的就算，不要存。要"prop 变化时重置"用 `key` prop。
- **`no-derived-state-effect`** / **`no-reset-all-state-on-prop-change`**:用 `<Inner key={resetKey} />` 包内层组件，React 自动重置 state，不要写 `useEffect(reset, [resetKey])`。
- **`no-cascading-set-state`** / **`no-chain-state-updates`**:不要一个 setState 触发 effect 再 setState。把相关 state 合并到一次更新，或者把逻辑挪进触发事件的 handler 里。
- **`no-event-handler`**:不要用 state+useEffect 模拟事件处理 (典型反模式是 "状态变化时调 prop 回调")。事件本身就在 handler 里直接处理。
- **`prefer-useReducer`**:同一组件 ≥4 个相互关联的 state，合并成 `useReducer`。
- **`no-adjust-state-on-prop-change`**:避免"prop 改了就 setState"。同样优先 `key` prop 或派生值。

### Component 边界

- **`only-export-components`**:`.tsx` 只 export 组件。非组件 export(常量、纯函数、`__test__` 助手) 挪到 `*-utils.ts` / `*-helpers.ts` / 邻近 `.ts` 文件，Fast Refresh 才生效。
- **`no-multi-comp`**:一个文件一个组件。shadcn 风格的多组件容器 (`Foo` + `FooTrigger` + `FooContent`) 拆成多文件 + barrel re-export，公共 API 不变。
- **`no-render-in-render`**:**不要在另一个组件的 body 里 `function Inner()` 或调用 `Inner()`**。Inner 组件 hoist 出去，把它依赖的闭包变量改成 props。`renderFoo()` 这种内部辅助函数也容易触发，改成真正的 `<Foo />` 子组件。
- **`no-many-boolean-props`**:≥4 个 boolean prop 聚合成一个对象 (`transferStatus: { isDownloaded, isTransferring, ... }`) 或换成状态枚举 (`status: 'idle' | 'transferring' | ...`)。
- **`no-giant-component`**:单组件不超过几百行，拆子组件。

### Accessibility / 语义

- **`button-has-type`**:每个原生 `<button>` 显式写 `type="button"`(默认是 `submit`,在 form 外会导致意外提交)。
- **`no-array-index-key` / `no-array-index-as-key`**:`key={index}` 只在 append-only + 不可变 + 没排序的列表里勉强可用。能用 `item.id` 就用 id;原始数据没 id 就在组件里维护一个并行的 `useRef<string[]>` 分配稳定 key，在 add/remove 时同步更新。
- **`control-has-associated-label`**:任何 `<input>` / `<select>` / `<textarea>` 必须有 visible label(`htmlFor`+`id`) 或 `aria-label` / `aria-labelledby`。
- **`click-events-have-key-events` + `no-static-element-interactions`**:`<div onClick>` 这种 static element 同时要 `role="button"`(或合适 role)、`tabIndex={0}`、`onKeyDown` 响应 Enter/Space。优先用语义元素 `<button>`,只在嵌套交互 (里面已经有 `<button>`) 时才用 div + role。
- **`no-noninteractive-element-interactions`**:`<p>` / `<h*>` / `<li>` 等非交互元素不要挂 `onClick`,改成交互元素或包一层 `<button>`。
- **`no-unknown-property`**:SVG 属性用 React camelCase(`strokeWidth`、`stopColor`、`fillRule`),不是 kebab-case。
- **`design-no-em-dash-in-jsx-text`**:JSX 文本里别用 `—`(看着像 AI 生成),换成 `-` / `:` / `;` / 括号。

### Tailwind 速记

- **`design-no-redundant-size-axes`**:`w-N h-N` 同值 → `size-N`(Tailwind v3.4+);`w-[24px] h-[24px]` → `size-[24px]`。
- **`design-no-redundant-padding-axes`**:`px-N py-N` 同值 → `p-N`。
- **`design-no-space-on-flex-children`**:flex 容器里的子元素不用 `space-x-*` / `space-y-*`,父容器加 `gap-*`。
- **`rendering-svg-precision`**:SVG path 小数点保留 1-2 位，4+ 位是无意义的精度膨胀。
- **`no-long-transition-duration`**:UI feedback 类 transition 控制在 1000ms 内。装饰性长时长动画 (>10s) 放在 CSS class / `@keyframes` 里，不要写成内联 `style={{ animation: 'x 12s ...' }}`(react-doctor 会扫到内联值并报警)。

### Bundle / Perf

- **`use-lazy-motion`**:用 `m.*` from `framer-motion` 替代 `motion.*`,在 App 根挂 `<LazyMotion features={domMax} strict>`。本项目已配 `domMax`(支持 `layoutId` 等高级特性)。
- **`js-flatmap-filter`** / **`js-combine-iterations`**:`.map(...).filter(Boolean)` 或 `.filter(...).map(...)` → `.flatMap(x => cond ? [v] : [])` 单遍迭代。
- **`js-tosorted-immutable`**:`[...arr].sort()` → `arr.toSorted()`(ES2023)。需要 `tsconfig.lib` ≥ `ES2023`。
- **`js-index-maps`**:循环里多次 `arr.find(...)` → 预先构造 `Map`,`map.get(id)` O(1)。
- **`js-set-map-lookups`**:循环里多次 `arr.includes(x)` → `Set`;子串匹配场景用单个正则比 `Array.some(s => str.includes(s))` 更优。
- **`jsx-no-constructed-context-values`**:`<Ctx.Provider value={{ ... }}>` 必须用 `useMemo` 包裹，避免每次渲染创建新对象。
- **`jsx-no-jsx-as-prop`**:`<Comp slot={<Inner />}>` 这种 JSX 作 prop，用 `useMemo` 或提取常量。

### 依赖管理

- **不要为了"防止以后用到"留依赖**。`unused-dependency` 会扣分。
- **shadcn/ui 走 umbrella 包 `radix-ui`**,不要单独装 `@radix-ui/react-*` 子包 (已经传递引入)。
- **删除 dependency 时检查 `radix-ui` umbrella 是否传递引入**,frontend 只用 umbrella 即可。
- **保留这些"看起来没用"的 devDep**(react-doctor 误报):
  - `react-doctor`、`react-grab`、`@react-grab/mcp` — 通过 npm 脚本 / 工具调用，不在 import 里。
  - `@wdio/local-runner`、`@wdio/mocha-framework`、`@wdio/spec-reporter` — 通过 `e2e/wdio.conf.mjs` 的 `runner` / `framework` / `reporters` 配置字符串引用，scanner 看不到。
  - `autocorrect-node` — `lint-staged` 配置里调 binary。
- **Tauri JS 插件**(`@tauri-apps/plugin-*`):只在前端真的 import 时才装。仅 Rust 侧用的 (autostart / global-shortcut / updater) 不需要 JS 包。

### shadcn UI 文件

- `*Variants` (cva 输出) 不要在组件文件里 export，要么挪到 `*-variants.ts`,要么不要 export(只在本文件用)。
- 旧的 `forwardRef` 包装风格按上面的 React 19 规范改写。

### 运行 react-doctor

- `bun run doctor` 跑全量。改完一组规则用 `npx react-doctor@latest --json --yes > /tmp/d.json` 拿结构化结果，按 `rule` 分组只看相关项。
- **得分公式**:`100 - 1.5 × |unique error rules| - 0.75 × |unique warning rules|`,**只数 unique 规则**。同一规则 100 处 = 1 处，所以"清完一个规则"才有分，部分修复不加分。优先选 **实例少** 的规则全消干净。
- 不要自动改 `.react-doctor/false-positives.md`,把误报候选告诉用户由用户决定。
