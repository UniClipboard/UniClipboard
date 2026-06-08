# syntax=docker/dockerfile:1
#
# Headless UniClipboard server node for VPS deployment (ADR-007).
#
# Two stages:
#   1. builder  — compiles the `uniclip` + `uniclipd` binaries as static musl
#                 executables. `uniclip start` spawns `uniclipd` as a child
#                 process even in `--foreground` mode, so both must be present.
#   2. runtime  — minimal Alpine image that runs the binary as a non-root
#                 user with HOME=/data anchoring all state on a mounted volume.
#
# Build context is the repository ROOT (see deploy/vps/docker-compose.yml),
# so paths below are repo-relative. Initialize submodules before building:
#   git submodule update --init --recursive

# ── builder ──────────────────────────────────────────────────────────────────
# uc-cli's dependency graph is ring + rustls (no openssl-sys / aws-lc-sys), so
# the musl target links fully statically with zero C-runtime dependencies — the
# resulting binary runs on the Alpine runtime stage (and anywhere else) as-is.
FROM rust:1.95-bookworm AS builder

# musl-tools provides musl-gcc, the static C linker that `ring`/`cc` invoke for
# the *-musl target. cmake + clang/libclang build aws-lc-sys (pulled into the
# uc-cli graph via aws-lc-rs). This mirrors .github/docker/build-bookworm.Dockerfile
# minus the GUI libs. `Acquire::Retries` rides out transient Debian mirror 5xx.
RUN echo 'Acquire::Retries "5";' > /etc/apt/apt.conf.d/80-retries \
 && apt-get update \
 && apt-get install -y --no-install-recommends musl-tools cmake clang \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# The whole Rust workspace lives under src-tauri/. The vendored iroh-blobs fork
# (src-tauri/vendor/iroh-blobs) is consumed via [patch.crates-io] and must be
# present in the context — that is why submodules must be initialized first.
COPY src-tauri/ ./src-tauri/
WORKDIR /build/src-tauri

# Pick the musl target matching the build host arch (native compile, no cross).
# Build and copy the binary out within a single RUN: the target/ dir is a
# cache mount (not persisted into the image), so the copy must happen here.
#
# On aarch64, GCC compiles the vendored C in libdbus-sys / libsqlite3-sys / etc.
# with __aarch64_*_sync outline-atomic helpers that live in libgcc. A static
# musl link with -nodefaultlibs cannot resolve them, so we tell GCC to inline
# the atomics (-mno-outline-atomics) via the cc crate's CFLAGS env. This is a
# no-op on x86_64 (the flag is aarch64-only and GCC there emits no such helpers).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/src-tauri/target \
    set -eux; \
    case "$(uname -m)" in \
      x86_64)  TARGET=x86_64-unknown-linux-musl ;; \
      aarch64) TARGET=aarch64-unknown-linux-musl; \
               export TARGET_CFLAGS="-mno-outline-atomics" \
                      CFLAGS_aarch64_unknown_linux_musl="-mno-outline-atomics" ;; \
      *) echo "unsupported build arch: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    rustup target add "$TARGET"; \
    cargo build --locked --release -p uc-cli -p uc-daemon --target "$TARGET"; \
    cp "target/$TARGET/release/uniclip" /uniclip; \
    cp "target/$TARGET/release/uniclipd" /uniclipd

# ── runtime ──────────────────────────────────────────────────────────────────
FROM alpine:3.20 AS runtime

# ca-certificates: TLS roots for outbound HTTPS (pairing rendezvous, etc.).
# busybox `wget` backs the compose healthcheck against the daemon control port.
RUN apk add --no-cache ca-certificates \
 && adduser -D -u 10001 -h /data uniclip \
 && mkdir -p /data \
 && chown uniclip:uniclip /data

COPY --from=builder /uniclip /usr/local/bin/uniclip
COPY --from=builder /uniclipd /usr/local/bin/uniclipd

# HOME=/data anchors ALL persistent state under the mounted volume — iroh
# identity, vault/keyslot, file-based KEK, sqlite db, blob cache and settings —
# so the node never needs re-pairing across restarts. See deploy/vps/README.md.
ENV HOME=/data
USER uniclip
WORKDIR /data

# Default: run the headless server daemon in the foreground as the container's
# main process. `start --server` sets UC_DAEMON_RUN_MODE=server (no system
# clipboard, no watcher); `--foreground` keeps it attached. The compose `app`
# service overrides this explicitly, and one-off provisioning commands replace
# it entirely (e.g. `docker compose run --rm app uniclip join`). No ENTRYPOINT
# so `docker compose run app uniclip <cmd>` works without arg duplication.
CMD ["uniclip", "start", "--server", "--foreground"]
