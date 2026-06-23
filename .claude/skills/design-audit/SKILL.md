---
name: design-audit
description: 定期审计代码库的工程设计问题（高心智复杂度、单一真相源被破坏、catch-all 胖接口、死代码、散落魔法字面量、泄漏抽象、资源生命周期靠环形缓冲）与可优化点，范围限定为自上次审计以来的 git churn，每条发现都落到 file:line 并对照本项目自己的 VISION.md / 各级 AGENTS.md / memory 规则，明确区分「意外复杂度」（要修）与「本质复杂度」（不动）。维护去重台账，重复运行只报新增。Use when 用户要做设计审计 / 每周设计复盘 / 技术债扫描，或运行 /design-audit；不用于行级 bug review（用 /code-review）或写功能。
---

# design-audit

每周一次的工程设计体检。复刻一套被验证有效的方法：**先理解，再批判；用「读起来难不难懂」当心智复杂度信号；每个结论落到真实代码；对照项目自己的规矩；只骂意外复杂度。**

详细的审计镜头、严重度定义、台账与报告格式、workflow 脚本骨架见 [REFERENCE.md](REFERENCE.md)。

## 何时触发

- 用户运行 `/design-audit`，或说"设计审计 / 每周复盘 / 技术债扫描 / 有哪些可优化的地方"
- **不是** 行级 bug review（那是 `/code-review`），**不是** 写功能

## 工作流（按序执行）

### 1. 读台账，定基线
- 读 `.planning/design-audit/ledger.md`。取 `last_audited_commit`（无则用 4 周前或 `main~200`）。
- 记下所有 `accepted` / `wontfix` 的 finding ID——本轮 **不再报** 这些。

### 2. 算 churn，定范围（核心：近期改动优先）
```bash
git log --oneline <last_audited_commit>..HEAD | head -50
git diff --stat <last_audited_commit>..HEAD
```
- 取改动文件，按 crate/子系统分组成 **审计目标**（每组 ≤ ~8 文件）。
- 叠加 `crates/AGENTS.md` 的 COMPLEXITY HOTSPOTS：churn 命中热点的目标优先级拉高。
- churn 为空 → 报告"无新改动"并退出。

### 3. 先理解，再批判（不可跳过）
- 对每个目标，先 **读关键文件** 建立局部地图，再判断。没有地图的批判是瞎猜。
- 准备好规矩参照系：`VISION.md`、相关 crate 的 `AGENTS.md`、`crates/uc-core/AGENTS.md §5.4`、
  memory 目录的规则（如 `[[no-timing-coupled-coordination]]`、`[[minimize-crate-api-surface]]`）。

### 4. 多 agent workflow 审计 + 对抗核实
- 调 `Workflow`（脚本骨架见 REFERENCE.md「Workflow skeleton」）：
  - **Audit 阶段**：每个目标一个 agent，按 REFERENCE.md 的 7 个镜头审，**每条 finding 必须带 file:line + 一段引用代码证据**。
  - **Verify 阶段**：对每条 finding 派对抗 agent 核实——是真的吗？是 **意外** 复杂度还是 **本质** 复杂度？违反了哪条项目规矩？默认怀疑，证据不足就降级/丢弃。
  - **Synthesize 阶段**：传入台账已知 ID 去重，按严重度排序，产出报告。

### 5. 落盘报告 + 更新台账
- 写 `.planning/design-audit/YYYY-Www.md`（报告格式见 REFERENCE.md）。
- 更新 `.planning/design-audit/ledger.md`：新 finding 追加为 `open`；记录本轮 `last_audited_commit = HEAD`。
- **不提交、不开 PR**（除非用户要）。最后在对话里给 P1/P2 的精简清单 + 推荐先动哪一条。

## 铁律（来自有效的那次）

- **落到代码**：没有 file:line + 代码证据的 finding 一律不写。宁可少报，不可臆测。
- **意外 vs 本质**：本质复杂度（问题本身就难，如剪贴板回环、分布式最终一致）**不报** 或只标注，只报实现引入的意外复杂度。
- **对照自家规矩**："违反你自己立的规矩"（VISION/AGENTS/memory）比"违反通用原则"更该优先。
- **去重**：台账里 accepted/wontfix 的不再报；同一 finding 跨周用稳定 ID 映射。
- **可执行**：每条 P1/P2 给出可切片的修法草图（S0/S1… 风格），不是空泛的"建议重构"。
