# PR #2243 CI repair

## Failure and scope

[Run 43384120262](https://github.com/UniClipboard/UniClipboard/actions/runs/43384120262)
failed `TestSandboxBudgets` in the Test and build job at PR head
`111ca871108aa9582503bfa662a69c77dcc9b25a`:

```text
sandbox_test.go:58: "while true do end": want instruction budget exhausted got deadline exceeded
```

The test used both the default 1,000,000-instruction budget and independent
200 ms wall deadline, but required instruction exhaustion to win. Race
instrumentation and scheduling delay can legitimately make the deadline win.
The VM implementation is unchanged from the v2 base
`2391bb1c271a8f8f3f3205342b928cbf3f3ec0bd`.

A normal local 30-repeat race run passed. A controlled 300 ms process suspension
reproduced the exact CI error in both the original PR and an independently
exported v2 base. The same competition also affected the byte-budget assertion
when only the first instruction test was shortened.

The existing `budgetErr` test helper now gives resource-budget assertions a
one-minute wall guard when no timeout was explicitly supplied. The dedicated
30 ms deadline assertion retains its explicit limit. The original resource
limits, exact error checks and broken-state checks remain intact. No production
limit, implementation, workflow, assertion or test case was removed.

## Reproduction

Build the existing race test executable, then suspend it while the budget test
runs. Run the same driver against the original and repaired source:

```sh
go test -race -c -o /tmp/pr2243-luavm.test ./internal/luavm
python3 - /tmp/pr2243-luavm.test <<'PY'
import os, signal, subprocess, sys, time
process = subprocess.Popen(
    [sys.argv[1], '-test.run=^TestSandboxBudgets$', '-test.v'],
    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    env={**os.environ, 'GORACE': 'atexit_sleep_ms=0'},
)
paused = False
for line in process.stdout:
    print(line, end='', flush=True)
    if line.startswith('=== RUN') and not paused:
        time.sleep(0.025)
        os.kill(process.pid, signal.SIGSTOP)
        time.sleep(0.3)
        os.kill(process.pid, signal.SIGCONT)
        paused = True
sys.exit(process.wait())
PY
```

The original PR and base fail with the error above. The repaired source passes
the same process replay. This isolates competing test limits; it does not replace
native application or mobile interaction acceptance.

## Validation

- Controlled process replay: failed before, passed after.
- `go test -race ./internal/luavm -count=100`: passed.
- `TMPDIR=/private/tmp go test -race ./...`: passed outside the socket-restricted sandbox.
- Documentation checks: passed. Remote CI is checked separately at the final PR head.

The full local check requires permission to create loopback TCP and Unix sockets.
A sandboxed attempt failed at socket creation; the full check is rerun outside
that sandbox with `TMPDIR=/private/tmp`. Those permission failures are separate
from the observed CI budget assertion.

Local verification artifact SHA-256 values:

- `pr2243-budget-paused-before.log`: `07d78eeab9a76a221fe5f7ef102fcddf3f0d2a35f3c39ac0ac615262a35475fd`.
- `pr2243-budget-paused-base.log`: `0c67d84c14a7f11f9255c282d35a5d6cc89bcfc47b0941901059c62f892db77b`.
- `pr2243-budget-paused-after.log`: `a636a33c3989ca86ba8e74098352c1ce462b24619d2b67700969115ec56c5586`.
- `pr2243-luavm-after.log`: `2662b945422b83509ecf1596318b27f28683eaa503ad766249c81906faa86610`.
- `pr2243-race-final.log`: `01ce10c67dfc5b7560dd0429d9cc473d6871f61ecda71d0a6cb90f5cf4436795`.
