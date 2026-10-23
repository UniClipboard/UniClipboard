#!/usr/bin/env bash
# Permit WebKit namespaces only in explicitly labelled disposable CI containers.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_ENVIRONMENT:-}" == github-hosted ]] || {
  echo 'This profile is only for disposable GitHub-hosted runners' >&2; exit 2;
}
profile_name=uniclipboard-webkit-ci
profile_file="$RUNNER_TEMP/$profile_name.apparmor"
cat > "$profile_file" <<PROFILE
abi <abi/4.0>,
include <tunables/global>
profile $profile_name flags=(unconfined) {
  userns,
}
PROFILE
sudo apparmor_parser --replace "$profile_file"
printf 'UC_WEBKIT_CI_APPARMOR_PROFILE=%s\n' "$profile_name" >> "$GITHUB_ENV"
# Keep the host restriction enabled. An unnamed unconfined container would
# transition into unprivileged_userns and lose network-namespace capabilities.
sysctl kernel.apparmor_restrict_unprivileged_userns
