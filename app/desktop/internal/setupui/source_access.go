//go:build !desktop_e2e

package setupui

import "github.com/uniclipboard/unic/internal/v1migration"

func defaultSourceDirectory() (string, error) { return v1migration.DefaultSource() }
func readSourceSecret(service, account string) ([]byte, error) {
	return v1migration.ReadSecret(service, account)
}
