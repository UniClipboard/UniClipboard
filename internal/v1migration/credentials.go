package v1migration

// credentialFailure contains only fixed platform guidance. It never contains
// service/account names, paths, native error strings, or secret bytes.
type credentialFailure struct{ reason string }

func (e *credentialFailure) Error() string {
	return "v1 migration: " + e.reason + "; supply the old passphrase"
}
func (e *credentialFailure) Unwrap() error { return ErrUnlock }

func credentialError(reason string) error { return &credentialFailure{reason: reason} }
