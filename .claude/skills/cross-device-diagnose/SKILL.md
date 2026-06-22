---
name: cross-device-diagnose
description: "Agent Loop for cross-device sync/transfer issues: collect environment fingerprints from BOTH machines first, check memory for known patterns, manage hypotheses with forced falsification, and persist evidence via /wrap. Orchestrates dual-side-debug + local-log-debug + systematic-debugging into a structured loop."
user-invocable: true
allowed-tools: Bash(git:*), Bash(ssh:*), Bash(sshpass:*), Bash(grep:*), Bash(rg:*), Bash(jq:*), Bash(cat:*), Bash(find:*), Bash(ifconfig:*), Bash(networksetup:*), Bash(scutil:*), Bash(netstat:*), Bash(lsof:*), Bash(pgrep:*), Bash(ps:*), Bash(mount:*), Bash(ping:*), Bash(curl:*), Bash(date:*), Bash(rm:*), Bash(wc:*), Bash(head:*), Bash(tail:*), Read, Edit, Write, AskUserQuestion, Agent
---

# cross-device-diagnose

## Purpose

An Agent Loop that orchestrates cross-device debugging with a disciplined, evidence-based process. It eliminates the recurring pattern where 50% of hypotheses are wrong, environment issues masquerade as code bugs, and debug context is lost across sessions.

**Observed anti-patterns (from 50 recent sessions):**
- Session S35: 25 prompts to diagnose LAN sync speed → root cause was BBR3 congestion controller, not code logic
- Session S29: 10 prompts chasing overlay address filtering → half spent on environment (Clash TUN, Tailscale)
- Session S22: 7 prompts on restore sync delay → ended up being iroh relay path, not application bug
- Session S28: 6 prompts on file sync failure → needed SSH password 3 times across retries

**The loop:**
```
1. Environment fingerprint (BOTH machines)  ← catches 40% of issues upfront
2. Memory pattern matching                  ← avoids re-investigating known issues
3. Hypothesis registration + prior ranking  ← parallel hypotheses, not serial guessing
4. Evidence collection (logs, state)        ← using existing tools
5. Forced falsification                     ← try to DISPROVE before concluding
6. Loop or escalate
```

## When to trigger

- `/cross-device-diagnose` — start the diagnostic loop
- User reports a sync, transfer, pairing, or connectivity issue between two devices
- Symptoms like "Mac sent but Windows didn't receive", "同步很慢", "文件/图片同步失败"
- Any issue where the problem might be on either end

## When NOT to use

- Single-machine issues → use `local-log-debug` + `systematic-debugging`
- Build/compilation errors → use `error-diagnose-fix`
- CI/PR failures → use `pr-greenlight`
- Just reading logs (no diagnosis needed) → use `dual-side-debug` directly

## State file

`/tmp/claude-xdd-state.json`:

```json
{
  "started_at": "ISO timestamp",
  "round": 0,
  "max_rounds": 5,
  "symptom": "Windows restore does not sync to Mac within 3s",
  "environment": {
    "mac": { "fingerprint_collected": true, "data": {} },
    "win": { "fingerprint_collected": true, "data": {} }
  },
  "memory_matches": [],
  "hypotheses": [
    {
      "id": 1,
      "description": "iroh relay path instead of direct LAN connection",
      "prior": "high",
      "status": "active|confirmed|ruled_out",
      "evidence_for": [],
      "evidence_against": [],
      "falsification_test": "check conn_type in logs for direct IP vs relay"
    }
  ],
  "evidence_ledger": [
    {
      "round": 0,
      "source": "mac-env-fingerprint",
      "observation": "Clash TUN active, 198.18.0.1 is default gateway",
      "hypothesis_impact": { "1": "supports" }
    }
  ],
  "ssh_config": {
    "host": "win",
    "needs_password": true
  }
}
```

## Phase 1 — Environment Fingerprint (MANDATORY FIRST STEP)

**Before looking at ANY logs or code**, collect environment fingerprints from both machines. This catches proxy/VPN/network issues that masquerade as application bugs.

### 1a — Mac fingerprint

Run all of these in parallel:

```bash
# Network interfaces and IPs
ifconfig | grep -E 'flags|inet ' | grep -B1 'inet '

# Default route
netstat -rn | grep default | head -3

# DNS configuration
scutil --dns | grep 'nameserver' | head -5

# Proxy/VPN detection
pgrep -lf 'clash|mihomo|v2ray|tailscale|wireguard|openvpn' 2>/dev/null
networksetup -getwebproxy Wi-Fi 2>/dev/null
networksetup -getsocksfirewallproxy Wi-Fi 2>/dev/null

# Check for TUN interfaces (198.18.x = Clash fake-ip, 100.x = Tailscale)
ifconfig | grep -A2 'utun\|tun' | grep inet

# Uniclipboard daemon status
pgrep -lf uniclip 2>/dev/null
.claude/skills/local-log-debug/uc-logs.sh status 2>/dev/null
```

### 1b — Windows fingerprint (via SSH)

```bash
ssh win "ipconfig & netstat -rn | findstr 0.0.0.0 & tasklist | findstr /i \"clash mihomo tailscale wireguard v2ray\" & netstat -an | findstr 42720"
```

If SSH fails or needs password, ask the user ONCE and note the config in state. Do not ask again.

### 1c — Analyze fingerprints

Build a structured assessment:

```
Environment Assessment:
  Mac:
    LAN IP: 192.168.1.100 (en0, Wi-Fi)
    Proxy: Clash TUN active (utun3, 198.18.0.1 gateway) ⚠️
    Tailscale: active (100.114.7.75 on utun4) ⚠️
    Daemon: running (PID 12345, profile=dev, log fresh)

  Windows:
    LAN IP: 192.168.1.129 (Ethernet)
    Proxy: Clash active (PID 5678) ⚠️
    Tailscale: not detected ✓
    Daemon: running (profile=dev)

  ⚠️ Both machines have Clash TUN active.
     Known issue: TUN mode hijacks UDP (198.18.0.1) and breaks iroh hole-punching.
     See memory: lan-sync-slow-tun-proxy-tailscale.md
```

### 1d — Check memory for matching patterns

Read the MEMORY.md index and scan for relevant entries:

```bash
grep -i "sync\|slow\|proxy\|tun\|tailscale\|relay\|transfer\|clipboard\|restore" \
  ~/.claude/projects/-Users-mark-MyProjects-uniclipboard/memory/MEMORY.md
```

For each match, read the memory file and check if the current symptom fits. Known patterns in this project:

| Memory | Pattern | Quick check |
|--------|---------|-------------|
| `lan-sync-slow-tun-proxy-tailscale.md` | LAN slow = both sides have TUN proxy | Check for 198.18.0.1 gateway |
| `presence-asymmetry-and-restart-red-herrings.md` | Text works but images don't = blob channel issue | Test text vs image separately |
| `mobile-sync-file-becomes-url.md` | File copied → URL received = outbound meta/file fork | Check entry type on sender |
| `issue1029-image-xpm-undecodable.md` | Image syncs "successfully" but can't paste = wrong MIME | Check image MIME in logs |
| `macos-text-dedup-permanent-swallows-recopy.md` | Re-copy same text = watcher dedup swallows it | Check for MEANINGFUL_REDEDUP |
| `iroh-production-perf-gotchas.md` | Hairpin NAT, CUBIC/BBR3, FsStore blocking | Check conn_type stability |

If a memory matches, report it immediately — it may short-circuit the entire investigation.

## Phase 2 — Hypothesis Registration

Based on the symptom + environment fingerprint + memory matches, register ALL plausible hypotheses at once (not just one):

```
Hypotheses (ranked by prior probability):

  H1 [HIGH] Clash TUN intercepting iroh UDP
     Prior: Both machines have TUN active; known issue in memory
     Falsification: Disable Clash on one side, retry

  H2 [MEDIUM] iroh selecting relay instead of direct LAN path
     Prior: conn_type logs previously showed relay/LAN oscillation
     Falsification: grep conn_type in logs, check if direct IP used

  H3 [LOW] Application-layer bug in restore dispatch
     Prior: Only if H1/H2 ruled out; restore logic was recently refactored
     Falsification: Check dispatch logs for error/skip/timeout
```

### Prior probability guidelines

| Prior | When to assign |
|-------|---------------|
| **HIGH** | Environment fingerprint shows a known issue, OR memory match is exact |
| **MEDIUM** | Symptom is consistent but environment looks clean; needs log evidence |
| **LOW** | Requires a code bug in recently-tested logic; unlikely but possible |

**Always test HIGH-prior hypotheses first.** This is the key efficiency gain — environment issues are caught before wasting rounds on code investigation.

## Phase 3 — Evidence Collection

### 3a — Quick tests for HIGH-prior hypotheses

For environment issues, run quick experiments first:

```bash
# H1: Can the machines reach each other directly on LAN?
ping -c 3 192.168.1.129

# H1: Is iroh using direct connection?
.claude/skills/dual-side-debug/dual-logs.sh grep "conn_type" --lines 20

# H2: What address is iroh connecting to?
.claude/skills/dual-side-debug/dual-logs.sh grep "connect selected path" --lines 10
```

### 3b — Log analysis for MEDIUM-prior hypotheses

Use the existing tools — don't hand-roll:

```bash
# Time-aligned view around the symptom
.claude/skills/dual-side-debug/dual-logs.sh merge --since "2026-06-21T10:00:00Z" --lines 400

# Filter to relevant subsystem
.claude/skills/dual-side-debug/dual-logs.sh query --filter '.target | test("sync|dispatch|transfer|restore")'

# Errors only
.claude/skills/dual-side-debug/dual-logs.sh query --filter '.level == "ERROR" or .level == "WARN"'
```

### 3c — Record every observation

Every piece of evidence goes into the ledger with its hypothesis impact:

```json
{
  "round": 1,
  "source": "dual-logs merge",
  "observation": "conn_type = Ip(100.79.191.42:56445) — Tailscale address, not LAN",
  "hypothesis_impact": {
    "H1": "strongly supports",
    "H2": "supports (relay not used, but wrong IP chosen)",
    "H3": "neutral"
  }
}
```

## Phase 4 — Forced Falsification

**Before declaring a root cause, actively try to disprove it.**

For each hypothesis marked as "supported by evidence":

1. **State the falsification test**: "If H1 is correct, then disabling Clash should immediately improve speed. If speed doesn't improve, H1 is wrong."

2. **Run the test** (or ask the user to run it if it requires their action):
   ```
   To test H1, please:
     1. Disable Clash on your Mac (quit the app or toggle TUN off)
     2. Restart the uniclipboard daemon
     3. Try the sync again
   
   If it's still slow after this, H1 is ruled out.
   ```

3. **Record the result**:
   - Falsified → mark hypothesis as `ruled_out`, never revisit
   - Survived → hypothesis is strengthened but still not proven
   - Need another round → register what additional evidence would prove/disprove it

### Falsification discipline

- A hypothesis is NOT confirmed just because evidence is consistent with it
- At least ONE attempt to disprove is required before confirming
- If the user provides the falsification result ("still slow after disabling Clash"), update the hypothesis immediately

## Phase 5 — Loop or Conclude

### Conclude (root cause found)

When a hypothesis has:
1. Multiple pieces of supporting evidence
2. Survived at least one falsification attempt
3. No contradicting evidence

→ Declare root cause with confidence level:

```
Root cause identified (HIGH confidence):

  H1: Clash TUN intercepting iroh UDP traffic
  
  Evidence:
    ✓ Both machines have TUN active (env fingerprint)
    ✓ iroh conn_type using 100.x Tailscale address instead of 192.168.x LAN
    ✓ Known pattern from memory (lan-sync-slow-tun-proxy-tailscale.md)
    ✓ Falsification survived: disabling Clash on Mac → sync improved to 17MB/s

  Recommended fix:
    - Short term: disable Clash TUN when using uniclipboard
    - Long term: filter Clash fake-ip (198.18.0.0/15) from iroh candidates
```

### Loop (need more evidence)

If no hypothesis is conclusive after a round:
- Increment round
- Re-rank hypotheses based on new evidence
- Promote MEDIUM → HIGH or demote HIGH → LOW based on evidence
- Collect more targeted evidence

### Escalate (stuck)

If stuck after 3 rounds (same hypotheses, no new evidence):

```
⚠️ Diagnosis inconclusive after 3 rounds.

  Active hypotheses:
    H2 [MEDIUM] iroh relay path — some evidence but not conclusive
    H3 [LOW] application bug — no evidence for or against

  Ruled out:
    H1 ✗ Clash TUN — falsified (still slow after disabling)

  Suggested next steps:
    A) Add diagnostic tracing to the suspect code path and reproduce
    B) Run a minimal reproduction (p2p-bench between the two machines)
    C) Escalate to iroh upstream (if the issue is in the networking layer)
```

### Max rounds (5): stop

```bash
rm -f /tmp/claude-xdd-state.json
```

Report all findings, ruled-out hypotheses, and remaining unknowns. Suggest whether to file an issue or continue in a focused session.

## Phase 6 — Persist via /wrap

When the session ends (user says "enough for now" or switches tasks), remind them to `/wrap`. The debug state from this skill's state file should be captured in `/wrap`'s `active-task.json` under the `debug` section:

```json
"debug": {
  "active": true,
  "symptom": "Windows restore → Mac sync delay ~3.4s",
  "hypotheses_tried": ["H1: Clash TUN (ruled out)", "H2: relay path (partially confirmed)"],
  "hypotheses_ruled_out": ["H1"],
  "evidence": ["conn_type=Ip(100.x)", "ping LAN=1ms", "Clash disabled no improvement"],
  "current_hypothesis": "H2: iroh candidate selection prefers Tailscale over LAN"
}
```

The next session's `/continue` will present this debug state, and this skill can resume from round N instead of restarting.

## SSH workflow

### First connection

Ask the user for SSH details exactly once:
```
To diagnose both sides, I need SSH access to the Windows machine.
  Host: win (or IP?)
  Password needed? (will not be stored in state file)
```

### Subsequent connections

Use `ssh win` (relies on `~/.ssh/config`). If password is needed:
```bash
sshpass -p "$WIN_PASS" ssh win "<command>"
```

**Never store the password in the state file or any persisted document.**

### Windows command gotchas

- Default shell is `cmd.exe`, not PowerShell
- Use `findstr` instead of `grep`
- Use `type` instead of `cat`
- Use `tasklist` instead of `ps`
- Paths use backslashes: `%LOCALAPPDATA%\app.uniclipboard.desktop-dev\logs\`
- For complex queries, use `powershell -Command "..."` explicitly

## Safety guardrails

- Environment fingerprint is MANDATORY before any hypothesis work
- Never confirm a root cause without at least one falsification attempt
- Never retry a ruled-out hypothesis (even across sessions via /wrap state)
- SSH password is never persisted — ask the user each session
- Max 5 rounds (cross-device debug is expensive in time)
- Don't modify code during diagnosis — this skill is read-only investigation
- If the root cause is environmental (not a code bug), say so clearly — don't force a code fix

## Relationship to other skills

| Skill | Role in this loop |
|-------|-------------------|
| `dual-side-debug` | **Tool**: fetches and merges logs from both machines |
| `local-log-debug` | **Tool**: reads single-machine logs |
| `systematic-debugging` | **Methodology**: Phase 4 falsification discipline comes from here |
| `/wrap` | **Persistence**: saves debug state for cross-session continuity |
| `/continue` | **Resume**: restores debug state to avoid re-investigating |
| `error-diagnose-fix` | **Not used**: that's for build errors, not runtime/sync issues |

## Anti-patterns

- Diving into code before collecting environment fingerprints
- Pursuing a single hypothesis serially (test H1 → fail → test H2 → fail → ...) instead of registering all hypotheses upfront and ranking by prior
- Declaring root cause without falsification ("evidence supports H1" ≠ "H1 is the root cause")
- Ignoring memory matches ("I know this looks like the TUN issue, but let me investigate from scratch")
- Asking for the SSH password more than once per session
- Dumping 200 lines of raw logs without interpretation
- Blaming code when the environment is the problem (and vice versa)
- Spending 5 rounds on a LOW-prior hypothesis while a HIGH-prior one was never tested
- Losing debug state across sessions because /wrap wasn't called
