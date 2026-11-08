package api

import (
	"context"
	"errors"
	"io"
	"mime"
	"net/http"
	"net/url"
	"strconv"
	"strings"
)

func (c *Client) ListHistory(ctx context.Context, cursor, tag string) (page HistoryPage, err error) {
	q := url.Values{"limit": {"50"}}
	if cursor != "" {
		q.Set("cursor", cursor)
	}
	if tag != "" {
		q.Add("tag", tag)
	}
	err = c.do(ctx, http.MethodGet, "/v1/history?"+q.Encode(), nil, &page)
	return
}
func (c *Client) UseHistory(ctx context.Context, id string) error {
	return c.do(ctx, http.MethodPost, "/v1/history/"+url.PathEscape(id)+"/use", nil, nil)
}
func (c *Client) HistoryPreview(ctx context.Context, item HistoryItem) (string, error) {
	part := -1
	for i, p := range item.Parts {
		if p.MIME == "text/plain" {
			part = i
			break
		}
	}
	if part < 0 {
		if item.Kind == "files" {
			for i, p := range item.Parts {
				if p.Kind == "file" {
					part = i
					break
				}
			}
		} else if item.Kind == "image" {
			return "图片", nil
		} else {
			return "富文本", nil
		}
	}
	if part < 0 {
		return "", nil
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "http://unic/v1/history/"+url.PathEscape(item.ID)+"/content?part="+strconv.Itoa(part), nil)
	if err != nil {
		return "", err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		return "", errors.New("cannot read history preview")
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		return "", errors.New("history content unavailable")
	}
	if item.Parts[part].Kind == "file" {
		_, params, _ := mime.ParseMediaType(resp.Header.Get("Content-Disposition"))
		return params["filename"], nil
	}
	b, err := io.ReadAll(io.LimitReader(resp.Body, 1024))
	if err != nil {
		return "", errors.New("cannot read history preview")
	}
	return strings.TrimSpace(strings.ToValidUTF8(string(b), "�")), nil
}
