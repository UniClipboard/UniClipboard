#!/usr/bin/env bash
set -uo pipefail

TIER="${1:-nightly}"
case "$TIER" in
  pr|nightly|release) ;;
  *)
    echo "usage: $0 [pr|nightly|release]" >&2
    exit 2
    ;;
esac

if [[ ! -f Cargo.toml ]]; then
  echo "run-membership-matrix.sh must run from the repository root" >&2
  exit 2
fi

ARTIFACT_DIR="${UC_E2E_ARTIFACT_DIR:-target/membership-e2e}"
mkdir -p "$ARTIFACT_DIR"
RESULTS="$ARTIFACT_DIR/results.tsv"
SUMMARY="$ARTIFACT_DIR/summary.md"
printf 'case\ttier\tclassification\tresult\n' > "$RESULTS"
printf '# Membership E2E matrix\n\n| Case | Classification | Result |\n| --- | --- | --- |\n' > "$SUMMARY"

FAILED=0

record_skip() {
  local case_id="$1"
  local classification="$2"
  printf '%s\t%s\t%s\tskipped\n' "$case_id" "$TIER" "$classification" >> "$RESULTS"
  printf '| %s | %s | skipped |\n' "$case_id" "$classification" >> "$SUMMARY"
}

run_case() {
  local case_id="$1"
  local classification="$2"
  local feature="$3"
  local test_target="$4"
  local test_name="$5"
  local effective_classification="$classification"
  local log_file="$ARTIFACT_DIR/${case_id}.log"
  local -a cargo_args=(
    test
    --manifest-path tests/e2e/Cargo.toml
  )

  if [[ "$TIER" == "release" ]]; then
    effective_classification="required"
  fi
  if [[ -n "$feature" ]]; then
    cargo_args+=(--features "$feature")
  fi
  cargo_args+=(--test "$test_target" "$test_name" -- --exact --ignored --nocapture)

  echo "==> $case_id ($effective_classification)"
  cargo "${cargo_args[@]}" 2>&1 | tee "$log_file"
  local test_status=${PIPESTATUS[0]}

  if [[ $test_status -eq 0 ]]; then
    printf '%s\t%s\t%s\tpassed\n' "$case_id" "$TIER" "$effective_classification" >> "$RESULTS"
    printf '| %s | %s | passed |\n' "$case_id" "$effective_classification" >> "$SUMMARY"
  elif [[ "$effective_classification" == "diagnostic" ]]; then
    printf '%s\t%s\t%s\tdiagnostic-failed\n' "$case_id" "$TIER" "$effective_classification" >> "$RESULTS"
    printf '| %s | %s | diagnostic failed |\n' "$case_id" "$effective_classification" >> "$SUMMARY"
  else
    printf '%s\t%s\t%s\tfailed\n' "$case_id" "$TIER" "$effective_classification" >> "$RESULTS"
    printf '| %s | %s | failed |\n' "$case_id" "$effective_classification" >> "$SUMMARY"
    FAILED=1
  fi
}

run_case C0 required "" membership_convergence c0_single_node_reports_complete
run_case C1 required "" membership_convergence c1_online_sponsor_chain_converges_and_syncs_directly

if [[ "$TIER" == "pr" ]]; then
  for case_id in C2-control C2 C3 H1 H2 H3 H4; do
    record_skip "$case_id" "not-in-pr-tier"
  done
else
  run_case C2-control diagnostic membership-diagnostics membership_convergence c2_control_recovers_when_joiner_stays_running
  run_case C2 diagnostic membership-diagnostics membership_convergence c2_offline_member_recovers_without_sponsor
  run_case C3 diagnostic membership-diagnostics membership_convergence c3_recovered_membership_survives_restart_without_duplicates

  if [[ -z "${UC_E2E_LEGACY_RELEASE_DIR:-}" ]]; then
    echo "UC_E2E_LEGACY_RELEASE_DIR is not set; H1-H4 cannot run" >&2
    for case_id in H1 H2 H3 H4; do
      record_skip "$case_id" "legacy-release-unavailable"
    done
    printf '\n> UC_E2E_LEGACY_RELEASE_DIR is not set; H1-H4 were required but could not run.\n' >> "$SUMMARY"
    FAILED=1
  else
    run_case H1 required historical-membership membership_compatibility h1_current_joiner_rejects_legacy_sponsor_without_partial_setup
    run_case H2 required historical-membership membership_compatibility h2_join_succeeds_after_legacy_sponsor_is_upgraded_in_place
    run_case H3 diagnostic historical-membership membership_compatibility h3_partial_upgrade_reports_waiting_for_upgrade
    run_case H4 required historical-membership membership_compatibility h4_final_legacy_upgrade_converges_without_repairing
  fi
fi

cat "$SUMMARY"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  cat "$SUMMARY" >> "$GITHUB_STEP_SUMMARY"
fi

exit "$FAILED"
