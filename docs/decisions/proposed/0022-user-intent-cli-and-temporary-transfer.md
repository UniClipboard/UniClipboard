# 0022: User-intent CLI and temporary transfer

- Status: Proposed. Design agreed with the maintainer on 2026-11-10. Not implemented.
- Date: 2026-11-10
- Requirement: Organize the `unic` command surface around what a user wants to do (send content, receive content). Support a temporary transfer to or from a peer that has no unic device identity, such as the browser page at `try.uniclipboard.app` or a plain tailcat peer.
- Related decisions: [0009 external channels](0009-external-channels.md), [0021 on-demand CLI service lifecycle](0021-cli-on-demand-service-lifecycle.md).
- Intended landing: [headless architecture](../../architecture/headless.md), [local service contract](../../contracts/local-service-contract.md), [Engine contract](../../contracts/engine-contract.md) (temporary transfer), [identity and trust](../../architecture/identity-and-trust.md) (ticket kinds).

## 1. Problem

The command list follows internal components. It has about 30 top-level commands. Several of them express one user intent (`submit` and `copy`; `addr`, `code`, `join`, `pair`, `approve`, `reject`, `requests`). Several others exist for development and diagnosis only (`submission`, `capabilities`, `paste`, `watch`, `diagnostics`, `cache`, `logs`).

A user can also not send content to a peer that is not a paired device. The browser transfer page uses the same tailcat network, but it speaks its own application protocol and its own ticket kind. See section 5.

## 2. Command surface

Two verbs cover the user's intent. Both have a short alias. Help lists these first.

| Command | Alias | Intent |
|---|---|---|
| `unic send` | `unic s` | Send content. |
| `unic recv` | `unic r` | Receive content. |

Other top-level groups, in the order help shows them:

| Command | Purpose |
|---|---|
| `unic history` (alias `h`) | Browse and manage local history. Unchanged. |
| `unic device` | Persistent trust: `add`, `list`, `remove`, `approve`, `reject`. Section 4. |
| `unic status`, `unic config`, `unic plugin`, `unic migrate` | Management. Unchanged. |
| `unic dev` | Development and diagnosis. Section 6. Not shown in the default help. |

There is no `web` command and no other new top-level command. A temporary transfer uses `send` and `recv`.
The project has not been released, so old command names are removed without aliases or migration.

## 3. `send` and `recv`

### 3.1 `unic send [CONTENT] [--to TARGET]`

Content comes from the argument, standard input, `-f FILE` (repeatable) or `--png PATH`. These are the existing `submit` inputs.

| Form | Behavior |
|---|---|
| `send --to DEVICE` | Send to one paired device through the normal sync path. `DEVICE` is a device ID or name. |
| `send --to all` | Send to every paired device. This replaces today's implicit broadcast. |
| `send --to ADDRESS` | Send to a full tailcat address as a temporary transfer. |
| `send` (no `--to`) | Start a temporary transfer. Print a 6-digit code and a link. Wait for one peer to take the content, then exit. |

Without `--to`, `send` no longer broadcasts to paired devices. Automatic sync of clipboard changes is unchanged. It is a different path from `send`.

A temporary transfer has these rules:

- It runs on a fresh tailcat listener with a random key. The listener is not the device address and it persists nothing.
- It accepts a connection on any virtual port. The browser page dials port 1; unic devices use 7777.
- It serves one peer. Later connections get `BUSY`, as in the browser protocol.
- It ends on delivery, on a timeout, on `Ctrl-C` or when the command is killed. No listener remains.
- The code expires 5 minutes after creation. It works once.

### 3.2 `unic recv [SOURCE]`

`SOURCE` is one of:

- a 6-digit code (`482913`, `482 913`, `482-913`);
- a full tailcat address (`tc…`);
- a link (`https://…#c=…&k=…`).

The command reads the ticket and chooses the flow. The user does not state the kind.

| Ticket kind | Flow |
|---|---|
| `uc-web-try` (code or link) | Dial the sender, pull the offered items, verify every SHA-256, acknowledge. Temporary transfer. |
| `unic-pair` (code) | Not a transfer. Print `this code pairs a device; use "unic device add"`. Exit with the usage code. |
| bare tailcat address | Dial the address and pull as a temporary transfer. |

Received items enter local history with origin `external:` and the source kind, as 0009 defines. They never create a device and they never gain trust.

`recv` with no argument lists transient received content. Whether `show` and `dismiss` stay as `recv` subcommands or move to `history` is settled during implementation, with the rule that no new top-level command is added.

## 4. `device` group

`device` replaces the pairing commands. The pairing protocol, user approval and trust rules do not change.

| New | Replaces |
|---|---|
| `device add [CODE or ADDRESS]` | `join`, `pair` |
| `device add` (no argument) | `code`, and shows `addr` |
| `device list` | `peers` |
| `device remove ID` | `unpair` |
| `device approve ID FINGERPRINT`, `device reject ID` | `approve`, `reject`, `requests` |

A device that joins through a code still needs user approval. This is the existing rule and it does not change.

## 5. Temporary transfer protocol

The browser page implements a framed pull protocol in `uc-website/src/lib/web-transfer/` (`frames.ts`, `protocol.ts`, `payload.ts`, `link.ts`, `short-code.ts`). unic implements the same protocol. It does not define a new one.

Facts verified from that code on 2026-11-10 (no live dial was done):

- Frame: `type(1) | length(4, big-endian) | payload`. Types: `HELLO`, `OFFER`, `BUSY`, `REJECT`, `DATA`, `END`, `ACK`, `CANCEL`. A `DATA` frame carries at most 64 KiB.
- Pull: the receiver dials, sends `HELLO pull` with the link token, receives `OFFER`, `DATA…`, `END`, verifies SHA-256 and sends `ACK`. The sender reports delivery only after `ACK ok`.
- Limits: 20 MiB per transfer and 50 items. Item kinds are text, image and file.
- Ticket: tailcat address plus a 128-bit token. The 6-digit code is an alias kept by rendezvous `/v1/web-pairings` for 5 minutes, with ticket kind `uc-web-try`.
- Network: both sides use tailcat and the default DERP map `https://tailcat.dev/derpmap.json`. The browser build pins tailcat `83921d71`. unic uses v0.7.0.
- The browser's short-code feature is behind `NEXT_PUBLIC_TRY_SHORT_CODE` and is off by default. Whether production enables it is not verified. Links work without it.

The protocol code lives in the Engine as an external channel (0009), not in the CLI. Hosts that have no CLI, such as the desktop GUI, can then offer the same capability. The CLI only prints the code and the link.

Constraints:

- Temporary transfer secrets (address, token, code, link) never appear in logs, events or error text. This follows the existing rule for pairing tickets.
- Received content is stored through the normal encrypted history path. Plaintext on disk is not allowed.
- The 20 MiB and 50-item limits apply to both directions, so unic and the browser agree on what is acceptable.
- The first dial of a restarted peer can take 5 to 11 s because of the stale-path issue in [0021 defect 4](../../exec/0021-cli-on-demand-service-lifecycle.md). A temporary listener uses a fresh key and a fresh process, so it is not affected on the sending side.

## 6. `dev` group

These commands move under `unic dev` and leave the default help:

`submission`, `capabilities`, `paste`, `watch`, `diagnostics`, `cache`, `logs`, and `copy` (writes the clipboard directly).

`dev` is a normal namespace, not a hidden mode. `unic dev help` lists it. Command requirements from 0021 stay attached to each leaf; only the names change.

## 7. Implementation order

1. Rename and regroup commands; keep behavior. Update the 0021 catalog, `CASES.md` and the test scripts. Verify with the existing end-to-end suites.
2. Add `send --to` forms for paired devices and `--to all`.
3. Engine temporary transfer: listener, `/try` protocol, receive path, history origin. A two-process end-to-end test runs unic against unic.
4. Cross-implementation check: unic receiving from the real browser build (headless browser or the pinned wasm), and the browser receiving from unic.
5. Ticket-kind dispatch in `recv` and the `unic-pair` refusal.

Each step keeps the previous end-to-end result working.

## 8. Open points

- Whether production `try.uniclipboard.app` enables the 6-digit code. If not, `send` prints only the link until it does.
- Where `recv show` and `recv dismiss` live (section 3.2).
- Whether the browser's reply (`HELLO push` after a delivered pull) is supported in the first version. This decision does not require it.
