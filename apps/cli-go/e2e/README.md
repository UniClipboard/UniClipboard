# Go CLI differential E2E

Rust/Go differential end-to-end checks for `apps/cli-go`. Each scenario runs
once with the Rust `uniclip` and once with the Go `uniclip`, both against the
same real `uniclipd`, each in a fresh isolated HOME and profile.

## Safety

Never run either CLI outside these tools. `isolated.py` sets a temporary HOME,
a unique `UC_PROFILE`, `UNICLIPBOARD_ENV=development` (file-based key storage,
no system keychain) and `UC_DISABLE_SYSTEM_CLIPBOARD=1` (the daemon never reads
or writes the system clipboard), and refuses the real HOME.

## Layout

| File | Purpose |
| --- | --- |
| `compat.py` | Runner: executes scenarios per flavor, normalizes volatile values, writes outputs and diffs |
| `scenarios.py` | Shared fixtures (`init_space`, `pair`) and lifecycle/argument scenarios |
| `scenarios_<group>.py` | Scenarios for one command group, loaded automatically |
| `dump_help.py` | Dumps `-h`/`--help` for every command path (help oracle) |
| `iso.py` | Runs one binary inside a shared isolated HOME for manual checks |

## Run

```sh
# Rust baseline and Go build side by side, each with a sibling uniclipd.
python3 apps/cli-go/e2e/compat.py --rust target/compat/rust --go target/compat/go \
  --out target/compat/run [--only SCENARIO ...]
```

`summary.tsv` lists `same`/`DIFF` per scenario; `<scenario>.diff` holds the
normalized unified diff, and `<scenario>.<flavor>.txt` the raw step records.
