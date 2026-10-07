# UniClipboard Tracing & Span 规范（v1）

**状态**：Active
**适用范围**：UniClipboard Desktop（Rust / Go host / Hexagonal Architecture）
**最后更新**：v1

---

## 1. 背景与目标

UniClipboard 是一个长期演进的跨平台应用，核心目标之一是 **可维护性与可演进性**。
为此，我们需要一套 **一致、低噪声、可聚合** 的可观测性规范。

本规范用于统一项目中对 [`tracing`](https://docs.rs/tracing) 的使用方式，明确：

- 哪一层可以创建 span
- span 的命名与字段规范
- 日志级别的使用边界
- tracing 与错误传播、async 并发的协作方式

---

## 2. 核心设计原则

### 2.1 Span 表达“业务语义边界”

- Span 表示 **一次业务行为的生命周期**
- 而不是函数进入 / 离开
- 也不是代码路径覆盖

> Span 的粒度 = Use Case 执行一次

---

### 2.2 日志追求“可理解性”，而非“完整性”

- 默认只记录 **有决策价值的信息**
- 禁止为了“以后可能用到”而提前打日志
- 日志的读者是未来的维护者，而不是机器

---

### 2.3 低基数优先（Low Cardinality First）

- Span / log 字段必须是低基数、可聚合的值
- 禁止记录：
  - 剪贴板内容
  - Settings 全量结构
  - 用户隐私数据
  - 大体积文本

---

## 3. 分层使用规范（强约束）

UniClipboard 使用 **Hexagonal Architecture**，tracing 的职责按层严格划分。

---

### 3.1 Use Case 层（`uc-app`）

**唯一允许创建业务 Span 的层级**

#### 允许

- 创建 use case 级 span
- 使用 `.instrument(span)` 包裹 async 执行
- 在 span 内记录关键业务状态

#### 禁止

- 在同一个 use case 中创建多个 span
- 创建 infra / 技术细节相关 span

#### 推荐模板

```rust
pub async fn execute(&self, input: Input) -> Result<Output> {
    let span = info_span!(
        "usecase.update_settings.execute",
        schema_version = input.schema_version
    );

    async move {
        info!("Start updating settings");

        // business logic

        info!("Settings updated successfully");
        Ok(())
    }
    .instrument(span)
    .await
}
```

---

### 3.2 Domain / Port 层（`uc-core`）

**零 Span 原则**

#### 严格禁止

- `info_span!`
- `debug_span!`
- `.instrument(...)`

#### 允许

- 默认不记录任何日志
- 极少数情况下使用 `trace!`（纯算法调试）

> Domain 层应保持：
> **纯逻辑、可测试、与运行时无关**

---

### 3.3 Infra / Platform 层（`uc-infra` / `uc-platform`）

**只记录技术行为，不定义业务边界**

#### 允许

- `debug!` / `info!`：技术行为
- `warn!` / `error!`：失败原因
- 自动附着到上层 span

#### 禁止

- 创建业务语义 span
- 决定 tracing 边界

#### 示例

```rust
info!(
    path = %config_path.display(),
    "Settings file saved"
);
```

---

### 3.4 传输适配层（daemon HTTP/WS 处理器与宿主命令处理器；旧 Tauri Command 层已退役）

**不创建业务 Span**

#### 原则

- Command 层是适配器
- 不拥有业务语义
- 不与 Use Case 抢 span 所有权

#### 允许

- `debug!` 打印参数
- 原样返回错误

---

## 4. Span 命名规范

### 4.1 格式

```
usecase.<use_case_name>.execute
```

### 4.2 示例

- `usecase.capture_clipboard.execute`
- `usecase.restore_clipboard_selection.execute`
- `usecase.update_settings.execute`

### 4.3 规则

- 全小写
- 使用点号分层
- 不包含实现细节（repo / sqlite / fs）
- 不包含技术名词

---

## 5. Span 字段规范

### 5.1 允许字段

| 类型   | 示例                          |
| ------ | ----------------------------- |
| 枚举   | `policy_version = "v1"`       |
| 布尔   | `encrypted = true`            |
| 小整数 | `item_count = 3`              |
| 状态   | `source = "system_clipboard"` |

---

### 5.2 禁止字段

| 类型                 | 原因        |
| -------------------- | ----------- |
| 剪贴板内容           | 隐私 / 安全 |
| Settings 全量        | 高基数      |
| 实际路径 / blob 内容 | 泄露风险    |
| UUID（非必要）       | 难以聚合    |

---

## 6. 日志级别约定

### `info!`

- Use Case 开始 / 成功结束
- 关键业务状态跃迁

### `debug!`

- 技术实现细节
- infra 行为

### `warn!`

- 用户输入错误
- 可恢复异常

### `error!`

- 不可恢复失败
- 数据不一致 / 损坏

---

## 7. 错误与 tracing

### 原则

- 错误通过 `Result` 传播
- tracing 负责记录上下文，不负责吞错

### 推荐模式

```rust
let result = do_something().await;

if let Err(ref e) = result {
    warn!(error = %e, "Operation failed");
}

result?;
```

---

## 8. Async / 并发规范

- 所有 use case async 执行必须使用 `.instrument(span)`
- 不允许手动跨线程传递 span
- 子任务默认继承父 span（由 tracing 自动处理）

---

## 9. 非目标（v1 不覆盖）

- 分布式 tracing（跨设备 / 网络）
- Metrics（Prometheus / OpenTelemetry metrics）
- 用户行为分析

---

## 10. 执行与约束

- 新增 Use Case **必须** 创建 span
- Code Review 必查：
  - 是否越层创建 span
  - 是否记录高基数字段
  - 是否泄露隐私数据

---

## 11. 一句话总结

> **Tracing 是 UniClipboard 的“业务时间线”，不是调试噪声。**
