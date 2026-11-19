package api

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"mime"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

// ExportHistory writes user-owned copies. Exclusive creation and os.Root prevent escape and overwrite.
func (c *Client) ExportHistory(ctx context.Context, id, dir string) (names []string, err error) {
	item, err := c.GetHistory(ctx, id)
	if err != nil {
		return nil, err
	}
	if err = os.MkdirAll(dir, 0700); err != nil {
		return nil, err
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return nil, err
	}
	defer func() { _ = root.Close() }()
	defer func() {
		if err != nil {
			for _, n := range names {
				_ = root.Remove(n)
			}
		}
	}()
	for i, p := range item.Parts {
		req, e := http.NewRequestWithContext(ctx, "GET", "http://unic/v1/history/"+url.PathEscape(id)+"/content?part="+strconv.Itoa(i), nil)
		if e != nil {
			return names, e
		}
		resp, e := c.hc.Do(req)
		if e != nil {
			return names, e
		}
		if resp.StatusCode != 200 {
			var v errBody
			_ = json.NewDecoder(resp.Body).Decode(&v)
			_ = resp.Body.Close()
			return names, &Error{Message: v.Error, Reason: v.Reason}
		}
		_, params, _ := mime.ParseMediaType(resp.Header.Get("Content-Disposition"))
		name := params["filename"]
		if name == "" {
			ext := ".bin"
			switch p.MIME {
			case "text/plain":
				ext = ".txt"
			case "text/html":
				ext = ".html"
			case "image/png":
				ext = ".png"
			}
			name = fmt.Sprintf("part-%d%s", i+1, ext)
		}
		if name == "." || name == ".." || filepath.Base(name) != name || strings.ContainsAny(name, "/\\\x00") {
			_ = resp.Body.Close()
			return names, fmt.Errorf("invalid export filename")
		}
		file, e := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			_ = resp.Body.Close()
			return names, e
		}
		names = append(names, name)
		_, e = io.Copy(file, resp.Body)
		_ = resp.Body.Close()
		if e == nil {
			e = file.Sync()
		}
		if ce := file.Close(); e == nil {
			e = ce
		}
		if e != nil {
			return names, e
		}
	}
	return names, nil
}
