# Trace Cloud process acceptance failure inventory

Write this inventory before implementing the driver. The seams are production
`unic` / `unicd`, the local service diagnostics API and a TLS OTLP receiver.

- A fresh profile may discover personal legacy history. Initialize with the
  production CLI in a private discovery home before starting the daemon.
- A wrong project, host or token may send spans to another target. Restrict
  hosted mode to the explicitly authorized existing project 722858 and verify
  its host and token with the authenticated connector before invoking it.
- A successful HTTP response may not prove ingestion. Keep upload acceptance
  separate from project-scoped root/child read-back.
- CLI sends may fail while the daemon still reports healthy exporters. Require
  all synthetic commands and the local unique commit/tree assertions to pass.
- Repeated request IDs may duplicate commit spans. Repeat one identical send;
  expect only three roots and three children for three unique commands.
- SDK environment Resource attributes may leak secrets. Seed local canaries
  and decode all OTLP fields before retaining local span artifacts.
- Disabled signals may upload unrelated telemetry. Permit only `/i/v1/traces`
  on the receiver and require the three other signals to remain disabled.
- A test-only build may masquerade as production. Build without test tags and
  record source hashes, binary hashes, build settings and the linker overrides.
- Source changes during acceptance may invalidate identity. Compare hashes
  before/after and fail on changes; repeat against the final frozen source.
- Early termination may lose spans or leave profile/binary state behind. Wait
  through the normal five-second batch window, shut down the daemon, remove
  its private workspace and retain a cleanup receipt with checksums.

Hosted execution requires the same driver to pass local TLS acceptance first.
No GUI, clipboard adapter, VM, production service or external resource is used.
