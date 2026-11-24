//go:build ios || android

package v1migration

import "context"

func ReadSecret(_, _ string) ([]byte, error) {
	return nil, credentialError("mobile system credentials must be supplied by the native host")
}

func ReadSecretContext(ctx context.Context, service, account string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return ReadSecret(service, account)
}
