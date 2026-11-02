# Findings
Research retained at .planning/research/diagnostic-performance/BRIEF.md.
Current Desktop HEAD and Engine pin match the research. Only research was untracked at intake.
Engine dedicated source worktree, branch feat/diagnostic-performance.
No Lody MCP tools are present in the available tool inventory.

## Verified vertical slice
- Engine 65e7472280de24ecd7786c0d7c410b25b9dc67f6 is remotely available on feat/diagnostic-performance.
- Native daemon export result: target/diagnostic-performance/daemon-run-1/result.json.
- First Collector test failed because the receiver defaults start_at=end. Fixed importer/config explicitly select beginning; disabled file name enrichment.
- Transport replay after correction delivered 8 metrics / 74 logs / 1 span, no send failures.
- Native backend run used Grafana 12.2.1, Prometheus 3.7.3, Loki 3.5.7, Jaeger 2.22.0, Collector Contrib 0.139.0. Query artifacts are in target/diagnostic-performance/backend-run-1; every owned backend exited gracefully.
- First ZIP embeds the earlier importer. Rebuild and re-export after fixing import configuration before declaring the bundled chain accepted.
- Native GUI acceptance is still running its original ten-minute expiry stage.
- Remaining product scope: GUI, Quick Panel and explicitly attributable WebView process registration/sampling; no whole-application coverage claim.

## Host coverage implementation
Authenticated auth/connect registration now binds role, PID, creation time and in-memory executable identity.
The resident GUI host additionally registers its owned UI child; Quick Panel uses its own client type with token caching preserved.
No process-name or PPID discovery is used. Wails beta.28 exposes internal WebView2 process APIs but no public attributable WebView PID API; unknown remains explicit.
process.memory.usage follows semconv 1.37.0 UpDownCounter, exported as cumulative non-monotonic Sum. CPU remains app-custom.
Current native receipts at 65e74722 precede host coverage and do not validate it; final build/run still required.
