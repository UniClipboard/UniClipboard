# Service application

`unic` is the CLI. `unicd` is the headless Engine owner. Neither program imports a UI toolkit.

```sh
go build -o dist/ ./app/service/cmd/unic ./app/service/cmd/unicd
UNIC_HOME=/absolute/private/state dist/unicd
echo hello | UNIC_HOME=/absolute/private/state dist/unic submit --json
```

See [headless architecture](../../docs/architecture/headless.md) and the [local service contract](../../docs/contracts/local-service-contract.md).
The optional `unic-migrate` program performs offline history import and is not included in the standard release archives.
