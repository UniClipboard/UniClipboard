# Trace failure inventory and acceptance seams

Written before implementation. The public seams are production `unic`, production
`unicd`, the local submission/configuration API, and the existing private TLS OTLP
receiver. No unit tests are added. Each next slice starts with process acceptance.

| Failure | Process assertion |
|---|---|
| Default silently disables traces | Omit the trace setting; require trace admission and one idempotent root/child tree at the synthetic 100% sample rate. |
| Either explicit switch leaks traces | Observe zero trace requests for more than the full five-second export period, including explicit trace false after restart. |
| Enabled submission is missing or duplicated | Decode one root and one child; repeat the same local request and require no second commit. |
| Caller metadata leaks | Supply synthetic content, path/request canaries and poisoned OTEL environment; reject canaries in complete decoded OTLP before retaining evidence. |
| Parent sampling differs | Offer a fixed workload; every exported root has exactly one child, one trace ID and the expected parent ID. |
| Exporter fails or hangs | Inject 401/429/503/delay; business commit remains successful and local finite failure state changes. |
| Queue grows or business waits | Hold receiver, offer enough submissions to overflow a fixed queue, assert drops and bounded process resources. |
| Disable returns before cancellation | Disable during delayed export; assert prompt completion and no trace request throughout the following export period. |
| Old spans cross generations | Hold pending work, disable/re-enable, then require only new admission counts. |
| Shutdown exceeds shared budget | Stop with slow trace and existing signals; process must terminate within the shared two-second flush budget plus process overhead. |
| Restart ignores saved settings | Restart the same explicitly initialized private profile and require the saved trace state and newly exported tree. |
| Resource leakage | Compare matched workloads and repeat switch cycles; retain CPU, RSS, handles, latency, HTTP request/body-byte measurements and cleanup results. |
| Hosted endpoint lacks traces | Require authorized target and official ingestion evidence before sending; otherwise record hosted NOT RUN. |

Trace scope is a unique explicit service submission and its Engine commit call.
It excludes device/plugin propagation, socket metadata, automatic paste analysis,
and implicit clipboard/Action/import roots. Those sources remain metrics/events.
