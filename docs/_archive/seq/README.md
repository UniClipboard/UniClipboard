# Seq Signal Definitions (Archived)

These saved searches were authored when UniClipboard exported spans and log events to a self-hosted Seq instance over OTLP/HTTP-protobuf.

The OTLP → Seq pipeline was retired in favor of Sentry Logs:

- Backend migration: commit `faa8eb8d` — `refactor(observability): migrate from OTLP/Seq to Sentry Logs`
- Frontend migration: issue [#543](https://github.com/UniClipboard/UniClipboard/issues/543)

The files are kept here as a historical reference for anyone who needs to reproduce the same queries against the archived Seq instance, or for porting the saved searches to Sentry's query language. They are not consulted by any tooling and may reference fields that no longer exist.
