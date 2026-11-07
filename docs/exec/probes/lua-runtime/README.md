# gopher-lua 上游行为探针

2026-11-07 在 macOS 隔离临时模块中执行 `go test -v -timeout 10s .`，四项通过。固定版本 v1.1.2，依赖摘要见 [go.sum.txt](go.sum.txt)。这些是上游行为检查，不是增强 VM 或沙箱安全验收。

| 探针 | 结果 |
|---|---|
| 纯 Lua 无限循环 + 20ms context | 收到错误并退出 |
| 原生 Go 函数等待 80ms + 10ms context | 越过截止时间，正常返回；宿主必须后置检查 |
| OpenBase 全局 | loadstring/print/collectgarbage/getfenv/setfenv/require/newproxy 存在 |
| 默认 OpenLibs | 无 utf8 全局 |

源文件使用 `.txt` 后缀以免参与项目构建，不引入嵌套模块或项目依赖。在仓库根执行以下命令可复制到临时目录复现（首次运行需要下载官方模块）：

```sh
probe_dir=$(mktemp -d /tmp/unic-lua-probe.XXXXXX)
cp docs/exec/probes/lua-runtime/go.mod.txt "$probe_dir/go.mod"
cp docs/exec/probes/lua-runtime/go.sum.txt "$probe_dir/go.sum"
cp docs/exec/probes/lua-runtime/probe_test.go.txt "$probe_dir/probe_test.go"
cd "$probe_dir"
env GOTOOLCHAIN=local GOMODCACHE="$probe_dir/mod" GOCACHE="$probe_dir/cache" GOPATH="$probe_dir/gopath" go test -v -timeout 10s .
```

测试源见 [probe_test.go.txt](probe_test.go.txt)，模块定义见 [go.mod.txt](go.mod.txt)。墙钟断言仅设置宽松的 1 秒上界，机器负载仍可能影响结果；失败需检查原因，不能改为忽略。
