# Service application instructions

Read [README](README.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
Keep `unic` and `unicd` free of direct and transitive UI toolkit dependencies. `unicd` may open the native desktop clipboard only after Desktop opts in and session admission succeeds.
Do not access the system clipboard or create a second Engine from a client.
Use production-binary acceptance: `make headless-e2e`. Preserve its manifest.
Do not log content, file paths, tickets or device addresses.
