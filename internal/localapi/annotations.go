package api

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/url"

	"github.com/uniclipboard/unic/engine"
)

// HistoryAnnotationAsset is a binary attachment read handle returned by the local service.
type HistoryAnnotationAsset struct {
	MIME string
	Size int64
	io.ReadCloser
}

// HistoryEnrichInput requests high-priority on-demand or manual enrichment.
type HistoryEnrichInput struct {
	PluginID string `json:"plugin_id,omitempty"`
	Force    bool   `json:"force,omitempty"`
}

// HistoryAnnotationBackend is implemented by service backends supporting derived annotations.
type HistoryAnnotationBackend interface {
	HistoryAnnotations(ctx context.Context, id string) ([]engine.HistoryAnnotation, error)
	HistoryAnnotationAsset(ctx context.Context, id, pluginID, annType, assetKey string) (HistoryAnnotationAsset, error)
	EnrichHistory(ctx context.Context, id string, req HistoryEnrichInput) error
}

// HistoryAnnotations returns persisted derived annotations for a history item.
func (c *Client) HistoryAnnotations(ctx context.Context, id string) ([]engine.HistoryAnnotation, error) {
	var out []engine.HistoryAnnotation
	err := c.do(ctx, http.MethodGet, "/v1/history/"+url.PathEscape(id)+"/annotations", nil, &out)
	return out, err
}

// HistoryAnnotationAsset opens one persisted binary attachment for an annotation.
func (c *Client) HistoryAnnotationAsset(ctx context.Context, id, pluginID, annType, assetKey string) (HistoryAnnotationAsset, error) {
	path := "http://unic/v1/history/" + url.PathEscape(id) +
		"/annotations/" + url.PathEscape(pluginID) +
		"/" + url.PathEscape(annType) +
		"/assets/" + url.PathEscape(assetKey)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, path, nil)
	if err != nil {
		return HistoryAnnotationAsset{}, err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		if isNoDaemon(err) {
			return HistoryAnnotationAsset{}, ErrNoDaemon
		}
		return HistoryAnnotationAsset{}, err
	}
	if resp.StatusCode != http.StatusOK {
		defer func() { _ = resp.Body.Close() }()
		var e errBody
		_ = json.NewDecoder(resp.Body).Decode(&e)
		return HistoryAnnotationAsset{}, &Error{Message: e.Error, Reason: e.Reason, Status: resp.StatusCode}
	}
	return HistoryAnnotationAsset{
		MIME:       resp.Header.Get("Content-Type"),
		Size:       resp.ContentLength,
		ReadCloser: resp.Body,
	}, nil
}

// EnrichHistory requests high-priority enrichment for an existing history item.
func (c *Client) EnrichHistory(ctx context.Context, id string, in HistoryEnrichInput) error {
	return c.do(ctx, http.MethodPost, "/v1/history/"+url.PathEscape(id)+"/enrich", in, nil)
}
