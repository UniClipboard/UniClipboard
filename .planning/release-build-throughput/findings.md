# 证据

- 发布 39900895332：54m41s，关键路径等待 34m21s；macOS x64 daemon 等待27m6s。
- 同时主线 Cache Warmup 39900863059 于04:32:29开始，发布于04:32:50开始。预热 macOS x64 daemon 于04:48:59启动，发布同类作业05:07:30才启动。
- 主线预热版本号变更触发了10个release组件、2个test、PR/E2E/coverage等。逻辑读取已优化但runner争用未处理。
- macOS x64 Cargo 8m53s，最后uniclipd动作320.27s；135 unportable-native-link绕过。Windows ARM Cargo17m21s，最终动作395.54s。
- 主线当前含GTK4迁移，Linux GUI Ubuntu24.04/glibc2.39容器与bookworm sidecar不同，需独立场景。
- Linux GUI stats全部unconsulted，run.sh未传MBX_CACHE_EXPORT_GROUP，build_daemon_release.sh普通cargo入口需验证是否真的使用native MBX wrapper。
