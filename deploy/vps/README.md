# UniClipboard headless server node — VPS deployment

Run UniClipboard headless on your own VPS as an always-online Space member. The
node syncs clipboard over iroh like any desktop peer and serves the mobile-sync
gateway to your phone behind a TLS reverse proxy. There is **no system
clipboard** — it is a relay/online member, not a desktop.

This stack is the implementation of
[ADR-007](../../docs/architecture/adr-007-headless-server-node-deployment.md).

## Topology

```text
          desktop peers (other networks)
                  │  iroh direct (UDP, RelayMode=Disabled)
                  ▼
   ┌─────────────────────────────────────────────┐  VPS
   │  app container (uniclip start --server)       │
   │   • iroh member  → published UDP :42999/udp   │◀── public internet
   │   • mobile_lan   → expose :42720 (internal)   │
   │   • HOME=/data   → volume `uniclip-state`     │
   └──────────────────────┬──────────────────────┘
                           │ http  app:42720  (internal Docker net only)
   ┌──────────────────────▼──────────────────────┐
   │  caddy container                              │
   │   • TLS termination + auto cert for $UC_DOMAIN│◀── phone https://$UC_DOMAIN
   │   • published :80 / :443                       │
   └─────────────────────────────────────────────┘
```

The plaintext `mobile_lan` port (`42720`) is **never** published to the host —
only Caddy's `80`/`443` and the iroh `42999/udp` port reach the public internet.

### Ports

| Port           | Where        | Purpose                                        |
| -------------- | ------------ | ---------------------------------------------- |
| `42999/udp`    | host public  | iroh direct connections from desktop peers     |
| `443` (+`/udp`)| host public  | HTTPS for the phone (Caddy → mobile_lan)        |
| `80`           | host public  | ACME challenge + HTTPS redirect (Caddy)         |
| `42720`        | internal only| `mobile_lan` plaintext HTTP (Caddy upstream)    |
| `42715`        | container    | daemon loopback control server (healthcheck)    |

## Prerequisites

- A VPS with a public IPv4 address and **Docker + Docker Compose v2**.
- A domain whose **A/AAAA record points at the VPS** (Caddy needs this to issue
  a certificate). DNS must resolve before you bring up Caddy.
- VPS firewall / security group open for: `80/tcp`, `443/tcp`, `443/udp`,
  `42999/udp`.
- An existing Space with another device online to pair with (you run `invite`
  on a desktop, and `join` here). This node *joins* — it does not create a Space.

## Image: pull or build

**Option A — pull the prebuilt image (recommended).** Every release publishes a
multi-arch (amd64 + arm64) image to the GitHub Container Registry, built by
`.github/workflows/build-server-image.yml`. No source checkout or compiler
needed — just this `deploy/vps/` directory and a `.env`:

```bash
docker compose pull
```

Track `:latest` by default, or pin a release in `.env` with
`UC_IMAGE=ghcr.io/uniclipboard/uniclipboard-server:vX.Y.Z`.

**Option B — build from source.** Requires the repository checked out **with
submodules** (the build needs the vendored `iroh-blobs` fork) and ~4 GB RAM:

```bash
git submodule update --init --recursive   # or: git clone --recursive <repo-url>
docker compose build
```

On a small droplet, build elsewhere and ship it — e.g.
`docker save ghcr.io/uniclipboard/uniclipboard-server:latest | ssh vps 'docker load'`,
or push to your own registry and set `UC_IMAGE` accordingly.

## 1. Configure

```bash
cd deploy/vps
cp .env.example .env
# edit .env: UC_DOMAIN, UC_PUBLIC_IP, UC_IROH_BIND_PORT
```

## 2. Provision (one-time, interactive — BEFORE the daemon)

`join` and the `mobile-sync` write commands refuse to run while a daemon is up,
so all provisioning happens in one-off containers first. They share the same
`uniclip-state` volume, so what they write is exactly what the long-running
daemon reads in step 3.

Make sure the image is available first — `docker compose pull` (Option A) or
`docker compose build` (Option B), per [Image: pull or build](#image-pull-or-build).

**a. Join the Space.** On a desktop already in the Space, run `uniclip invite`
(or use the desktop UI) to get an invitation code, then:

```bash
docker compose run --rm app uniclip join
```

This prompts interactively for the **invitation code** and the **Space
passphrase** (run it from an interactive terminal). For a fully non-interactive
run you can pass them as flags instead — note these land in your shell history:

```bash
docker compose run --rm app uniclip join --code <CODE> --passphrase <PASSPHRASE>
```

**b. Enable the mobile-sync gateway for the public domain.** `--advertise-url`
makes the phone's install URL/QR point at Caddy instead of a LAN IP:

```bash
docker compose run --rm app \
  uniclip mobile-sync lan enable \
  --advertise-url https://${UC_DOMAIN:-clip.example.com} \
  --accept-network-risk
```

**c. Register your phone.** Mints credentials and prints the install QR/URL:

```bash
docker compose run --rm app uniclip mobile-sync devices add --label "My iPhone"
```

Scan the QR (or open the printed `https://<domain>` URL) in the SyncClipboard /
UniClipboard mobile client. Repeat for each phone.

## 3. Start the stack

```bash
docker compose up -d
```

`app` runs `uniclip start --server --foreground` as its main process (headless,
no system clipboard). Caddy starts once `app` reports healthy, issues the
certificate for `UC_DOMAIN`, and begins proxying.

## 4. Verify

```bash
# Daemon healthy and in server mode:
docker compose ps                       # app should be "healthy"
docker compose logs -f app

# TLS + gateway reachable from the public internet (401 = listener up, auth
# required — that is expected without credentials):
curl -i https://<your-domain>/SyncClipboard.json

# The plaintext port is NOT exposed on the host (should fail / refuse):
curl -i http://<your-domain>:42720/SyncClipboard.json
```

Acceptance walk-through:

- Copy something on a paired desktop on another network → it lands here over
  iroh (visible in `docker compose logs app`).
- On the phone, pull the latest clipboard and push new content → it fans out to
  the desktops through Caddy.

## State & backups

Everything needed to keep identity + membership + mobile credentials lives under
`HOME=/data` on the `uniclip-state` volume, so the node **never re-pairs** across
restarts. Under `/data/.local/share/app.uniclipboard.desktop/`:

| State                         | Path                                  |
| ----------------------------- | ------------------------------------- |
| iroh identity (node secret)   | `iroh-identity/` (file secure storage)|
| file-based KEK                | `keyring/`                            |
| keyslot + device id           | `vault/keyslot.json`, `vault/device_id.txt` |
| database                      | `uniclipboard.db`                     |
| iroh blob cache               | `iroh-blobs/blobs.db`                 |
| settings (mobile creds, LAN)  | `settings.json`                       |

The daemon falls back to file-based secure storage automatically (no D-Bus /
keyring on a headless box), so the iroh secret and KEK are on the volume — keep
the volume and you keep the node.

Back up both named volumes — `uniclip-state` (re-pairing if lost) and
`caddy-data` (TLS certificates / ACME account; losing it forces re-issuance and
risks Let's Encrypt rate limits):

```bash
docker run --rm -v uniclip-state:/data -v "$PWD":/backup alpine \
  tar czf /backup/uniclip-state.tgz -C /data .
```

## Operations

```bash
docker compose logs -f app             # follow daemon logs
docker compose restart app             # restart the daemon (state preserved)
docker compose down                    # stop the stack (volumes preserved)
docker compose pull && docker compose up -d    # update (Option A: prebuilt image)
docker compose up -d --build           # update (Option B: rebuild from source)
```

Updates keep the `uniclip-state` volume, so no re-pairing — a new image just
swaps the binary. To move to a pinned release, bump `UC_IMAGE` in `.env` before
`docker compose pull`.

To change the advertised domain or add devices later, **stop the daemon first**
(`docker compose down`), rerun the relevant `docker compose run --rm app
uniclip mobile-sync …` command, then `docker compose up -d` — the write commands
still refuse to run while the daemon is up.

## Security notes

- `mobile_lan` is plaintext HTTP + Basic Auth, designed for trusted LANs. On the
  public internet it is **only** reachable through Caddy's TLS — keep `42720` on
  the internal network (this compose does; do not add a host port mapping for it).
- There is no relay fallback (`RelayMode::Disabled`). If a desktop's network
  blocks outbound UDP to `UC_IROH_BIND_PORT`, it cannot reach this node directly.
- The healthcheck assumes no `UC_PROFILE` is set (daemon control port `42715`).
  If you set `UC_PROFILE`, the control port changes — update the healthcheck.

## Troubleshooting

- **`setup not complete` on `up -d`** — provisioning (step 2a `join`) did not
  persist to the volume. Confirm you ran the `docker compose run` commands
  against this same project/volume, then retry.
- **Caddy cannot get a certificate** — DNS for `UC_DOMAIN` must resolve to this
  VPS and `80`/`443` must be open before `up -d`. Check `docker compose logs caddy`.
- **Phone gets the wrong URL** — re-run step 2b with the correct
  `--advertise-url https://<domain>` (daemon stopped), then `up -d`.
- **Desktop on another network won't connect** — verify `42999/udp` is open in
  the VPS firewall and that `UC_PUBLIC_IP` in `.env` is the real public IP.
