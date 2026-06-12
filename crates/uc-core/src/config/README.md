# Config Module - Pure Data Only

## Purpose / 目的

This module contains **pure data structures** for application configuration.
此模块包含应用配置的**纯数据结构**。

## Iron Rules / 铁律

1. **No validation** / 无验证
   - Empty strings are valid / 空字符串是合法的
   - Zero values are valid / 零值是合法的

2. **No default value logic** / 无默认值逻辑
   - Callers decide defaults / 调用者决定默认值

3. **No business logic** / 无业务逻辑
   - This is a DTO only / 这只是一个 DTO

## Examples / 示例

```rust
use uc_core::config::AppConfig;

// Empty config is valid (fact, not error)
let config = AppConfig::empty();

// From TOML - missing fields become empty values
let config = AppConfig::from_toml(&toml_value)?;
```

## Migration Note / 迁移说明

This replaces the old `settings` concept which contained policy.
这替代了包含策略的旧的 `settings` 概念。
