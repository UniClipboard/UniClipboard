//go:build !desktop_e2e

package setupui

import (
	"context"

	"github.com/uniclipboard/unic/internal/v1migration"
)

func defaultSourceDirectory() (string, error) { return v1migration.DefaultSource() }
func readSourceSecret(ctx context.Context, service, account string) ([]byte, error) {
	return v1migration.ReadSecretContext(ctx, service, account)
}
