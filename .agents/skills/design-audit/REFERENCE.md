# design-audit — REFERENCE

## 7 个审计镜头（rubric）

每个 audit agent 对一个目标逐镜头过一遍。**每条 finding 必须带 file:line + 一段引用代码**，并标注严重度与（若有）被违反的项目规矩。

| # | 镜头 | 信号 / 怎么看 | 本项目坐标 |
|---|------|--------------|-----------|
| L1 | **心智复杂度 / 读起来难懂** | 「为一个概念维护 N 个互相兜底的机制」「读懂任一处必须在脑中同时持有另外几处」「同一函数靠 mode flag 串多条语义不同的流程」。问自己：要理解这段，得同时记住几件事？ | 时间窗叠层、origin 分支、跨 worker 共享可变单例 |
| L2 | **单一真相源被破坏** | 同一事实存两份：配置写的值 vs 实际 bind 的值；状态在 orchestrator/UI/handler 各存一半 | `uc-application/AGENTS.md §10.2`；mobile 端口 vs current_status |
| L3 | **catch-all 胖接口** | 一个 port/trait 混 query+command+field-patch；调用方依赖了用不到的方法；死方法堆在 trait 上 | `uc-core/AGENTS.md §5.2`「保持最小接口」；端口拆分先例 |
| L4 | **死代码 / 投机泛化** | `#[allow(dead_code)]` 当「未来功能」供着；零调用方的 pub；留着的旧逻辑无移除计划 | 根 `AGENTS.md`「不保留无移除计划的并行旧逻辑」 |
| L5 | **散落魔法字面量 / 时间耦合** | 同一物理量的 timeout 散在多文件；隐含的「A 必须 > B」关系只活在注释里 | memory `[[no-timing-coupled-coordination]]`；`uc-daemon-process::timing` 范式 |
| L6 | **泄漏抽象 / 边界污染** | port doc 提上层模块/路由/协议名；infra 类型上浮到 core/app；adapter 长成 orchestrator | `uc-core/AGENTS.md §5.4`；`uc-infra/AGENTS.md §4.1` |
| L7 | **资源生命周期 / 可优化** | 靠环形缓冲淘汰代替显式释放；无界缓存；每次重算可缓存的；孤儿无 TTL 清理；重复序列化；阻塞主/watcher 线程 | `uc-infra/AGENTS.md §13`；`IncomingMobileBuffer` 先例 |

### 跨镜头铁律
- **意外 vs 本质**：先判定这复杂度是不是问题本身固有（剪贴板回环、跨 iroh+DB 无事务的最终一致 = 本质）。本质的 **不报** 或仅标注「justified」；只报实现引入的意外复杂度。
- **对照自家规矩优先**：能引到 VISION/AGENTS/memory 具体条目的 finding，severity 与可信度都更高。
- **给修法**：P1/P2 必须带可切片的修法草图（参考 self-write 重构的 S0/S1 切片：先抽常量→删死方法→收口接口，每片单一边界、行为保持优先）。

## 严重度

| 级别 | 含义 | 处置 |
|------|------|------|
| **P1** | 严重违反工程设计 + 高心智负担，阻碍理解；或违反项目明文铁律 | 报告置顶，给切片方案，建议尽快排期 |
| **P2** | 真实技术债，值得排期，但不阻塞 | 报告列出 + 修法草图 |
| **P3** | 小瑕疵 / nice-to-have | 报告附录一行带过 |
| **OPT** | 纯优化机会（性能/资源），非设计缺陷 | 单独分节，标注预期收益与成本 |

## 稳定 finding ID（去重关键）

格式：`<crate-or-area>:<file-basename-no-ext>:<symbol-or-area>:<lens>`
例：`uc-application:coordinator:write:L5`、`uc-application:cleanup:check_device_quota:L4`

同一问题跨周必须解析到同一 ID，台账才能去重。symbol 优先用稳定的函数/类型名，避免行号（行号会漂）。

## 台账 `.planning/design-audit/ledger.md`

```markdown
# Design Audit Ledger

last_audited_commit: <full-sha>
last_audited_at: <YYYY-MM-DD>

| id | severity | lens | title | status | first_seen | last_seen | note |
|----|----------|------|-------|--------|-----------|-----------|------|
| uc-application:coordinator:write:L5 | P1 | L5 | echo 时间窗散落字面量 | fixed | 2026-W25 | 2026-W25 | S0 33f4ace2c |
| uc-application:cleanup:check_device_quota:L4 | P2 | L4 | 配额死代码当未来功能 | open | 2026-W25 | 2026-W26 | |
```

- `status` ∈ `open | accepted | wontfix | fixed`。
- 新一轮：命中已有 ID 且 status∈{accepted,wontfix} → **跳过不报**；status=open → 更新 last_seen；status=fixed 但又出现 → 标 `regressed` 重报（P1）。
- 全新 ID → 追加 `open`。
- 跑完更新 `last_audited_commit = HEAD`、`last_audited_at`。
- 用户对某条说「接受/不修」→ 把该行改成 accepted/wontfix（以后不再烦）。

## 报告 `.planning/design-audit/YYYY-Www.md`

```markdown
# 设计审计 YYYY-Www

**范围**: <last_sha>..<HEAD>（N commits, M 文件）
**目标分组**: <子系统列表>
**新增 finding**: P1×a P2×b P3×c OPT×d（已跳过台账已知 e 条）

## P1 — 严重
### [id] 标题
- **位置**: `path/file.rs:line` (`symbol`)
- **证据**: <一段引用代码 / 为什么读起来难懂：要同时记住哪几件事>
- **意外而非本质**: <为什么这是实现引入的，不是问题固有>
- **违反**: <VISION/AGENTS/memory 具体条目，若有>
- **修法草图**: <S0/S1 风格切片，单一边界、行为保持优先>

## P2 — 值得排期
（同上精简）

## P3 — 小瑕疵
- 一行带过

## OPT — 优化机会
- `path:line` <收益 / 成本>

## 本质复杂度（已审，判定不动）
- <列出审过但判为 justified 的，避免下轮重审 + 让 reviewer 放心>
```

## Workflow skeleton

scout 阶段（churn 分组）在 skill 主体里 inline 做完，把 `targets` 和台账 `knownSkipIds` 作为 `args` 传入。脚本按 REFERENCE 的 rubric/schema fan-out。

```js
export const meta = {
  name: 'design-audit',
  description: 'Audit recent-churn targets for accidental-complexity design smells, verify, dedup, rank',
  phases: [{ title: 'Audit' }, { title: 'Verify' }, { title: 'Synthesize' }],
}

const { targets, knownSkipIds, ruleRefs } = args // ruleRefs: VISION/AGENTS/memory 路径清单
const FINDING = { type:'object', additionalProperties:false,
  required:['id','severity','lens','title','file','symbol','evidence','accidental_why','violates','fix_sketch'],
  properties:{
    id:{type:'string'}, severity:{enum:['P1','P2','P3','OPT']}, lens:{type:'string'},
    title:{type:'string'}, file:{type:'string',description:'repo-rel path:line'},
    symbol:{type:'string'}, evidence:{type:'string',description:'引用代码/为什么难懂'},
    accidental_why:{type:'string',description:'为何是意外而非本质复杂度；若判本质则写 justified'},
    violates:{type:'string',description:'VISION/AGENTS/memory 条目或 none'},
    fix_sketch:{type:'string'},
  }}
const FINDINGS = { type:'object', additionalProperties:false, required:['findings'],
  properties:{ findings:{type:'array', items:FINDING} } }
const VERDICT = { type:'object', additionalProperties:false, required:['id','is_real','is_accidental','keep'],
  properties:{ id:{type:'string'}, is_real:{type:'boolean'}, is_accidental:{type:'boolean'},
    keep:{type:'boolean'}, reason:{type:'string'} } }

const rubric = `按 7 镜头审：L1 心智复杂度/难懂、L2 单一真相源、L3 catch-all 胖接口、L4 死代码/投机泛化、`
  + `L5 散落字面量/时间耦合、L6 泄漏抽象/边界污染、L7 资源生命周期/可优化。`
  + `铁律：先读代码建图再判；每条 finding 必带 file:line + 引用代码；区分意外 vs 本质（本质判 justified）；`
  + `对照项目规矩(${ruleRefs.join(', ')})；P1/P2 给可切片修法。stable id=<area>:<file>:<symbol>:<lens>。`

const results = await pipeline(
  targets,
  t => agent(`${rubric}\n\n审计目标「${t.label}」，文件：\n${t.files.join('\n')}`,
             { label:`audit:${t.label}`, phase:'Audit', schema:FINDINGS, agentType:'Explore' }),
  (res, t) => parallel((res?.findings||[]).map(f => () =>
    agent(`对抗核实这条 finding（默认怀疑，证据不足就 keep=false）：${JSON.stringify(f)}\n`
        + `判定：is_real（代码真这样？）、is_accidental（意外复杂度而非问题固有？本质则 false）、`
        + `keep（值得进报告？）。可重读 ${f.file} 求证。`,
        { label:`verify:${f.id}`, phase:'Verify', schema:VERDICT })
      .then(v => ({ ...f, verdict:v }))))
)
const confirmed = results.flat().filter(Boolean)
  .filter(f => f.verdict?.keep && f.verdict?.is_real && f.verdict?.is_accidental)
  .filter(f => !knownSkipIds.includes(f.id)) // 台账 accepted/wontfix 去重

const report = await agent(
  `把以下已核实 finding 合成设计审计报告（Markdown），按 P1>P2>P3>OPT 排序，`
  + `每条含 位置/证据/意外而非本质/违反/修法草图；末尾列「本质复杂度（已审不动）」。\n`
  + `FINDINGS:\n${JSON.stringify(confirmed, null, 2)}`,
  { label:'synthesize', phase:'Synthesize' })

return { report, confirmed, skipped: knownSkipIds.length }
```

## 规模建议

- targets ≤ 6、churn 小 → 直接跑；targets 多 → 按子系统切，优先 COMPLEXITY HOTSPOTS 命中的。
- 对抗核实是质量关键：宁可 Verify 阶段砍掉一半「看着像但其实是本质复杂度」的 finding，也不要污染报告。
- 报告只进 `.planning/design-audit/`（`.planning/` 是开发自留路径，不受语言审查、CodeRabbit 已跳过）。
