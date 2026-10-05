package commands

// Human labels for daemon enum values (snake_case wire names).

var recoveryStateLabels = map[string]string{
	"not_required":                "not required",
	"awaiting_passphrase":         "awaiting passphrase",
	"recovering":                  "recovering",
	"recovered":                   "recovered",
	"partially_recoverable":       "partially recoverable",
	"failed":                      "failed",
	"admission_recovery_required": "admission recovery required",
}

var admissionCategoryLabels = map[string]string{
	"credential_missing":         "credential missing",
	"authentication_mismatch":    "authentication mismatch",
	"current_metadata_invalid":   "current metadata invalid",
	"legacy_fallback_invalid":    "legacy fallback invalid",
	"legacy_migration_failed":    "legacy migration failed",
	"record_relation_incomplete": "record relation incomplete",
	"derived_summary_invalid":    "derived summary invalid",
	"generation_mismatch":        "generation mismatch",
	"other_storage_error":        "storage error",
}

var admissionStageLabels = map[string]string{
	"credential":          "credential",
	"repository_metadata": "repository metadata",
	"legacy_repository":   "legacy repository",
	"repository_record":   "membership records",
	"recovery_summary":    "membership summary",
	"storage":             "local storage",
}

var admissionActionLabels = map[string]string{
	"restore_credential":    "restore credential from a trusted copy",
	"choose_backup":         "choose a known-good backup",
	"rebuild_derived_state": "rebuild derived state",
	"export_diagnostics":    "export diagnostics",
}
