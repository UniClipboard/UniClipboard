# 依赖审计例外的适用边界

审计仍检查完整锁文件。例外只适用于已证明不可达的漏洞路径。
`scripts/architecture/check-cargo-audit-exceptions.mjs` 在 PR 的 Rust 检查中验证这些边界。
边界改变时必须重新评审，不能仅保留忽略编号。

## 加密整数运算

Engine 实际使用的 `crypto-bigint` 已升级至 `0.7.10`，`ctutils` 已升级至 `0.4.7`。
它们包含对应的时序侧信道修复。

`crypto-bigint 0.5.5` 仍由 `jsonwebtoken 10.4.0` 的 ECDSA 后端引入。
Desktop 的 JWT 签名与验证只选择 `HS256`，并使用 HMAC 密钥；不会调用该后端的秘密整数运算。
该旧依赖没有可直接兼容替换的修复版本。

因此，`RUSTSEC-2026-0387` 只对这个不可达后端保留例外。
保护脚本拒绝未修复的其他 `crypto-bigint` 版本、旧依赖的新调用方，以及 RSA、ECDSA 或 EdDSA JWT 算法。
旧版依赖消失时，保护脚本要求删除或重新评审例外。

既有 `RUSTSEC-2023-0071` 例外同样依赖只使用 HMAC JWT 的事实。

## 构建期 Swagger 归档

`zip 1.1.4` 只属于 `utoipa-swagger-ui 7.1.0` 的构建依赖。
当前 `utoipa 4` 集成没有可直接兼容升级至 `zip 9` 的路径。
该构建使用 `vendored` 功能，输入来自 `utoipa-swagger-ui-vendored 0.1.2` 的固定归档。
它不处理用户上传或网络下载的 ZIP 内容。

已核对归档中的 1097 个条目：没有符号链接，也没有 SUID 或 SGID 权限位。
因此，`RUSTSEC-2026-0367`、`RUSTSEC-2026-0368` 和 `RUSTSEC-2026-0369`
所需的攻击者控制归档输入在当前构建路径中不可达。

保护脚本验证以下条件：

- `zip 1.1.4` 只能被该 Swagger 包通过构建依赖使用。
- Swagger 包版本与 `vendored` 功能必须保持不变。
- 构建脚本和内置归档必须匹配已评审的 SHA-256。
- 依赖版本、使用方或归档改变时必须重新评审例外。

## 验证命令

```bash
node scripts/architecture/check-cargo-audit-exceptions.mjs
cargo audit --file Cargo.lock
```

## 外部依据

- [crypto-bigint 时序问题](https://rustsec.org/advisories/RUSTSEC-2026-0387.html)
- [ZIP 特权权限位问题](https://rustsec.org/advisories/RUSTSEC-2026-0367.html)
- [ZIP 符号链接目标问题](https://rustsec.org/advisories/RUSTSEC-2026-0368.html)
- [ZIP 经符号链接设置权限的问题](https://rustsec.org/advisories/RUSTSEC-2026-0369.html)
