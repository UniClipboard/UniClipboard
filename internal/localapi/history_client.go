package api

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"net/url"
	"strconv"
	"strings"

	"github.com/uniclipboard/unic/engine"
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
	if item.Kind == "directory" {
		return "目录快照", nil
	}
	if item.Kind == "directory_reference" {
		return "源目录引用；发送时读取当前内容", nil
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

// QueryHistory lists records matching q, newest first.
func (c *Client) QueryHistory(ctx context.Context, q HistoryQuery) (page HistoryPage, err error) {
	v := url.Values{}
	if q.Limit > 0 {
		v.Set("limit", strconv.Itoa(q.Limit))
	}
	if q.Cursor != "" {
		v.Set("cursor", q.Cursor)
	}
	for _, k := range q.Kinds {
		v.Add("kind", k)
	}
	for _, o := range q.Origins {
		v.Add("origin", o)
	}
	for _, t := range q.Tags {
		v.Add("tag", t)
	}
	if q.DeviceID != "" {
		v.Set("device", q.DeviceID)
	}
	if q.FromMs != 0 {
		v.Set("from", strconv.FormatInt(q.FromMs, 10))
	}
	if q.BeforeMs != 0 {
		v.Set("before", strconv.FormatInt(q.BeforeMs, 10))
	}
	err = c.do(ctx, http.MethodGet, "/v1/history?"+v.Encode(), nil, &page)
	return
}

func (c *Client) GetHistory(ctx context.Context, id string) (it HistoryItem, err error) {
	err = c.do(ctx, http.MethodGet, "/v1/history/"+url.PathEscape(id), nil, &it)
	return
}

// HistoryContent opens one history part without buffering its body.
// Close the result to release the service-owned content lease.
func (c *Client) HistoryContent(ctx context.Context, id string, part int) (HistoryContent, error) {
	req, err := http.NewRequestWithContext(ctx, "GET", "http://unic/v1/history/"+url.PathEscape(id)+"/content?part="+strconv.Itoa(part), nil)
	if err != nil {
		return HistoryContent{}, err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		if isNoDaemon(err) {
			return HistoryContent{}, ErrNoDaemon
		}
		return HistoryContent{}, err
	}
	if resp.StatusCode != http.StatusOK {
		defer func() { _ = resp.Body.Close() }()
		var e errBody
		_ = json.NewDecoder(resp.Body).Decode(&e)
		return HistoryContent{}, &Error{Message: e.Error, Reason: e.Reason, Status: resp.StatusCode}
	}
	_, params, _ := mime.ParseMediaType(resp.Header.Get("Content-Disposition"))
	return HistoryContent{MIME: resp.Header.Get("Content-Type"), Name: params["filename"], Size: resp.ContentLength, ReadCloser: resp.Body}, nil
}

// CopyHistoryContent streams part number part of a record to w.
func (c *Client) CopyHistoryContent(ctx context.Context, id string, part int, w io.Writer) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, "http://unic/v1/history/"+url.PathEscape(id)+"/content?part="+strconv.Itoa(part), nil)
	if err != nil {
		return err
	}
	resp, err := c.hc.Do(req)
	if err != nil {
		if isNoDaemon(err) {
			return ErrNoDaemon
		}
		return err
	}
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusOK {
		var e errBody
		_ = json.NewDecoder(resp.Body).Decode(&e)
		if e.Error == "" {
			e.Error = resp.Status
		}
		return &Error{Message: e.Error, Reason: e.Reason}
	}
	_, err = io.Copy(w, resp.Body)
	return err
}

func (c *Client) UpdateHistory(ctx context.Context, id string, u HistoryUpdate) (it HistoryItem, err error) {
	err = c.do(ctx, http.MethodPatch, "/v1/history/"+url.PathEscape(id), u, &it)
	return
}

func (c *Client) DeleteHistory(ctx context.Context, id string, revision uint64) error {
	return c.do(ctx, http.MethodDelete, "/v1/history/"+url.PathEscape(id)+"?revision="+strconv.FormatUint(revision, 10), nil, nil)
}

// Actions lists what plugins offer on text records.
func (c *Client) Actions(ctx context.Context) ([]Action, error) {
	var out struct {
		Actions []Action `json:"actions"`
	}
	err := c.do(ctx, http.MethodGet, "/v1/actions", nil, &out)
	return out.Actions, err
}

// RunAction creates a new history item without applying it to the clipboard.
func (c *Client) RunAction(ctx context.Context, id, key string) (HistoryItem, error) {
	var out struct {
		Item HistoryItem `json:"item"`
	}
	err := c.do(ctx, http.MethodPost, "/v1/history/"+url.PathEscape(id)+"/actions/"+key, nil, &out)
	return out.Item, err
}

func (c *Client) RestoreHistory(ctx context.Context, id, request string) (out engine.RestoreSyncResult, err error) {
	body := struct {
		RequestID string `json:"request_id"`
	}{request}
	err = c.do(ctx, http.MethodPost, "/v1/history/"+url.PathEscape(id)+"/use", body, &out)
	return
}

func (c *Client) HistoryTags(ctx context.Context) (tags []string, err error) {
	err = c.do(ctx, http.MethodGet, "/v1/history/tags", nil, &tags)
	return
}
