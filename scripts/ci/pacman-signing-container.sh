#!/usr/bin/env bash
# Only run on a disposable GitHub-hosted Linux runner: swap is disabled globally.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]] || {
  echo 'requires a disposable GitHub-hosted runner' >&2; exit 2;
}
mode="${1:?expected test or sign}"
[[ "$mode" == test || "$mode" == sign ]] || exit 2
sudo swapoff --all
[[ "$(wc -l < /proc/swaps)" == 1 ]] || { echo 'active swap remains' >&2; exit 1; }
docker build --tag uc-pacman-signing - <<'DOCKERFILE'
FROM archlinux:base-devel
RUN pacman -Syu --noconfirm --needed python git && pacman -Scc --noconfirm
DOCKERFILE
container=(docker run --rm --tmpfs /run/pacman-gpg:rw,nosuid,nodev,noexec,mode=0700
  --env GNUPGHOME=/run/pacman-gpg -v "$PWD:/work:ro" -w /work)
if [[ "$mode" == test ]]; then
  "${container[@]}" uc-pacman-signing python scripts/tests/pacman_signing_acceptance.py
else
  [[ -n "${PACMAN_GPG_PRIVATE_KEY:-}" && "${FINGERPRINT:-}" =~ ^([A-F0-9]{40}|[A-F0-9]{64})$ ]]
  packages=(built/*.pkg.tar.zst)
  [[ "${#packages[@]}" == 1 && -f "${packages[0]}" ]]
  mkdir signed
  # Stdin avoids writing the private key into Docker's persistent container config.
  printf '%s' "$PACMAN_GPG_PRIVATE_KEY" | "${container[@]}" -i -v "$PWD/signed:/out" \
    uc-pacman-signing python scripts/ci/pacman-signing-secrets.py sign \
    "/work/${packages[0]}" /out/snapshot --arch x86_64 --fingerprint "$FINGERPRINT"
fi
