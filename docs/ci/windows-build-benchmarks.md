# Windows x64 构建对照（2026-09-09）

## 采用结论

- 采用快速测试模式：手动构建默认 `test`，正式发布与可复用调用默认 `release`。
- 不采用应用自身产物缓存。完整命中后仍重编相同的应用 crate，单次耗时差不足以证明稳定收益。
- 不改变后台与 GUI 的顺序，不升级 runner。
- 测试缓存独立预热并纳入容量保护，避免新分支因没有共享测试缓存而反复从头编译。

## 方法与边界

全部使用 `windows-latest`、Windows x64、相同应用源码，先验证快速模式，再验证 workspace 缓存。
每组对照固定提交；没有把无缓存构建与命中缓存的构建直接作为优化收益对照。
总耗时包含工具安装、缓存恢复、前端依赖、编译、打包、上传，受 runner 和网络波动影响。
以下数字是本次观测，不是每次构建的时长保证。

`test` 通过单次 Cargo 环境覆盖使用 `opt-level=1`、`lto=off`、`codegen-units=16`；
仍走 release 打包路径，不启用 debug assertions，不改变 panic 或调试符号配置。
这类测试包用于功能验证，不替代正式版运行性能测试。

## 第一组：编译优化

提交 `efb1425f7`，应用源码与主分支基线 `6dc638139` 相同。

| 方式 | 缓存 | 整轮 | 后台步骤 | GUI 与打包步骤 | 构建记录 |
| --- | --- | --- | --- | --- | --- |
| 正式优化 | 依赖完整命中 | 23m24s | 10m07s | 5m50s | [A](https://github.com/UniClipboard/UniClipboard/actions/runs/34329179303) |
| 快速测试，首次 | 未命中，保存依赖缓存 | 22m39s | 11m20s | 5m32s | [B](https://github.com/UniClipboard/UniClipboard/actions/runs/34329181074) |
| 快速测试，重复 | 依赖完整命中 | 8m47s | 1m57s | 3m16s | [C](https://github.com/UniClipboard/UniClipboard/actions/runs/34331388945) |

A 与 C 都重编后台 9 个、GUI 8 个 crate，编译数量没有变化，优化参数减少了实际编译成本。
A 的缓存恢复为 113s，C 为 45s；因此整轮差值不应全部归因于编译参数。

## 第二组：应用自身产物缓存

提交 `c08253838`，编译参数与第一组的 `test` 完全相同。

| 方式 | 整轮 | 后台步骤 | GUI 与打包步骤 | 构建记录 |
| --- | --- | --- | --- | --- |
| 仅依赖缓存，完整命中 | 7m59s | 1m41s | 3m04s | [D-off](https://github.com/UniClipboard/UniClipboard/actions/runs/34332537638) |
| 首次生成 workspace 缓存 | 23m33s | 不作为命中对照 | 不作为命中对照 | [D-seed](https://github.com/UniClipboard/UniClipboard/actions/runs/34332538962) |
| workspace 缓存完整命中 | 7m01s | 1m17s | 2m46s | [E](https://github.com/UniClipboard/UniClipboard/actions/runs/34335044592) |

缓存由 1,311,087,968 bytes 增至 1,366,195,580 bytes，增加 55,107,612 bytes（约 4.2%）。
E 日志确认 `cache-workspace-crates: true` 和 `full match: true`，但仍重编与 D-off 完全相同的后台 9 个、GUI 8 个 crate。
没有实现跳过应用编译的目标，单次 58s 差异不足以排除 runner 波动，因此移除实验开关，继续只缓存依赖。
未据此声称仅修改页面时可以复用应用产物，也没有引入修改源码时间戳等额外机制。

## 默认构建发现的额外失配

最终默认测试 [F](https://github.com/UniClipboard/UniClipboard/actions/runs/34336799091) 正确选择了快速模式，
但没有命中缓存：实际使用的 Rust 1.95.0 未变，runner 额外预装的 stable 从 1.98.0 变为 1.98.1，
使环境匹配段从 `92a3a808` 变为 `eb4ef0e0`，依赖段 `4e1e2363` 不变。旧缓存仍存在。
这轮验证被主动取消以读取日志，没有把它当作成功的命中验证。

rust-cache 会枚举并哈希所有已安装工具链。修复在 GitHub 临时构建机器上确认并保留项目指定的活跃工具链，
删除未使用的其他工具链；不关闭编译器匹配检查，不强行复用旧缓存，也不修改本机或自托管环境。
切换后的新缓存需要先生成一次，再验证重复构建。

提交 `1ce64cb73` 的 [G1](https://github.com/UniClipboard/UniClipboard/actions/runs/34339353967)
完整构建成功（23m31s），保存了 1,311,366,316 bytes 的新缓存。
同提交 [G2](https://github.com/UniClipboard/UniClipboard/actions/runs/34341733713)
未指定构建模式，正确使用默认 test，完整命中并成功完成（8m45s）。
两轮缓存匹配前后只包含 Rust 1.95.0，环境段均为 `bd69162e`，G2 日志明确记录 `full match: true`。
工具链隔离、模式选择和缓存规则共 23 项测试通过；实际编译器和正式优化配置均未改变。

## 包体积与验证

| 产物 | 正式优化 | 快速测试 |
| --- | --- | --- |
| NSIS 安装包 | 19,579,389 bytes | 23,325,177 bytes |
| Portable ZIP | 24,681,278 bytes | 31,446,685 bytes |
| GUI EXE | 15,000,576 bytes | 19,639,808 bytes |
| Daemon EXE | 34,773,504 bytes | 51,873,280 bytes |

已下载 A/B 产物并核对，快速 portable ZIP 完整性检查通过，包含 GUI、daemon、portable.dat 和 README。
未进行 Windows 原生交互或运行性能对照。构建模式与缓存规则由脚本测试和 actionlint 验证。

## 参考

- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [rust-cache 的缓存范围与 workspace 说明](https://github.com/Swatinem/rust-cache)
