#!/usr/bin/env bash
# Sourced by every CI compiler entry, including native descendants. Snapshot
# transport alone may load the separate restricted R2 identity file.
unset BUILD_CACHE_R2_ENDPOINT BUILD_CACHE_R2_ACCESS_KEY_ID BUILD_CACHE_R2_SECRET_ACCESS_KEY
for uc_mbx_remote_variable in ${!MBX_REMOTE_@}; do
  unset "$uc_mbx_remote_variable"
done
unset uc_mbx_remote_variable
