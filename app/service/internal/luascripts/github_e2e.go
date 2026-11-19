//go:build unic_e2e

package luascripts

import (
	"errors"
	"net/http"
	"net/url"
	"os"
)

type fixtureTransport struct {
	endpoint *url.URL
}

func (f fixtureTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	req = req.Clone(req.Context())
	host := req.URL.Host
	if host != "api.github.com" && host != "codeload.github.com" && host != "github.com" {
		return nil, errors.New("unexpected fixture host")
	}
	req.URL.Scheme, req.URL.Host = f.endpoint.Scheme, f.endpoint.Host
	req.URL.Path = "/" + host + req.URL.Path
	req.URL.RawPath = ""
	req.Host = f.endpoint.Host
	return http.DefaultTransport.RoundTrip(req)
}

func init() {
	if endpoint, err := url.Parse(os.Getenv("UNIC_E2E_GITHUB")); err == nil && endpoint.Scheme == "http" && endpoint.Hostname() == "127.0.0.1" {
		githubHTTP.Transport = fixtureTransport{endpoint}
	}
}
