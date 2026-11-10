package observability

import (
	"context"
	"net/url"
	"strings"
)

// Release assembly may set these using -ldflags -X. The token is ingestion-only,
// extractable from the binary, and never an administrative credential.
var PostHogHost string
var PostHogToken string
var ProductVersion = "dev"

func releaseEndpoint() string {
	if PostHogHost == "" {
		return ""
	}
	return strings.TrimRight(PostHogHost, "/") + "/i"
}

// ReleaseOptions binds the embedded token to its exact release destination.
// An explicit endpoint override never forwards the embedded project token.
func ReleaseOptions(endpoint string) Options {
	o := Options{Version: ProductVersion}
	if endpoint != "" && endpoint == releaseEndpoint() {
		o.Token = func(context.Context) (string, error) { return PostHogToken, nil }
	}
	return o
}
func analyticsHost(endpoint string) string {
	u, err := url.Parse(endpoint)
	if err != nil {
		return ""
	}
	u.Path = ""
	u.RawPath = ""
	return u.String()
}
func postHogEndpoint(endpoint string) bool {
	u, err := url.Parse(endpoint)
	return err == nil && strings.TrimRight(u.Path, "/") == "/i"
}
