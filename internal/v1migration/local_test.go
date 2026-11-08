package v1migration

import (
	"bytes"
	"context"
	"database/sql"
	"fmt"
	"image"
	"net/url"
	"os"
	"strings"
	"testing"
)

// Explicit opt-in: never reads personal data or credentials in normal tests.
func TestLocalSource(t *testing.T) {
	root := os.Getenv("UNIC_V1_TEST_SOURCE")
	if root == "" {
		t.Skip("set UNIC_V1_TEST_SOURCE to check a local source")
	}
	s, err := Open(context.Background(), root, "UniClipboard", nil, ReadSecret)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	count, err := s.Count()
	if err != nil {
		t.Fatal(err)
	}
	good, bad, missing := 0, 0, 0
	unavailableKinds, missingKinds := map[string]int{}, map[string]int{}
	imageDiagnostics := map[string]int{}
	fileDiagnostics := map[string]int{}
	err = s.Walk(context.Background(), func(r *Record, err error) error {
		var kind string
		if queryErr := s.tx.QueryRow("SELECT content_category FROM clipboard_entry WHERE entry_id=?", r.ID).Scan(&kind); queryErr != nil {
			return queryErr
		}
		if err != nil {
			bad++
			unavailableKinds[kind]++
		} else {
			good++
		}
		missing += r.MissingParts
		missingKinds[kind] += r.MissingParts
		if err != nil && kind == "file" {
			rows, queryErr := s.tx.Query("SELECT line_index,original_text_ct,size_bytes FROM entry_file_set WHERE entry_id=? AND kind='file'", r.ID)
			if queryErr != nil {
				return queryErr
			}
			type file struct {
				index int64
				size  sql.NullInt64
				ct    []byte
			}
			var files []file
			for rows.Next() {
				var f file
				if queryErr = rows.Scan(&f.index, &f.ct, &f.size); queryErr != nil {
					_ = rows.Close()
					return queryErr
				}
				files = append(files, f)
			}
			queryErr = rows.Err()
			_ = rows.Close()
			if queryErr != nil {
				return queryErr
			}
			for _, f := range files {
				path, e := s.openFileColumn(f.ct, fmt.Sprintf("uc:file_set_line:v1|%s|%d|original-text", r.ID, f.index))
				if e != nil {
					fileDiagnostics["invalid_metadata"]++
					continue
				}
				if u, e := url.Parse(path); e == nil && u.Scheme == "file" {
					path = localFilePath(u)
				}
				st, e := os.Stat(path)
				if os.IsNotExist(e) {
					fileDiagnostics["file_absent"]++
				} else if e != nil {
					fileDiagnostics["file_unreadable"]++
				} else if !st.Mode().IsRegular() || f.size.Valid && st.Size() != f.size.Int64 {
					fileDiagnostics["file_changed"]++
				} else {
					fileDiagnostics["file_present"]++
				}
			}
		}
		if r.MissingParts > 0 || err != nil {
			rows, queryErr := s.tx.Query(`SELECT r.id,r.event_id,r.mime_type,r.inline_data,r.blob_id,r.size_bytes FROM clipboard_snapshot_representation r JOIN clipboard_entry e ON e.event_id=r.event_id WHERE e.entry_id=? AND r.mime_type LIKE 'image/%'`, r.ID)
			if queryErr != nil {
				return queryErr
			}
			type rep struct {
				id, event, mime string
				ct              []byte
				blob            sql.NullString
				size            int64
			}
			var reps []rep
			for rows.Next() {
				var p rep
				if queryErr = rows.Scan(&p.id, &p.event, &p.mime, &p.ct, &p.blob, &p.size); queryErr != nil {
					_ = rows.Close()
					return queryErr
				}
				reps = append(reps, p)
			}
			queryErr = rows.Err()
			_ = rows.Close()
			if queryErr != nil {
				return queryErr
			}
			for _, p := range reps {
				var b []byte
				var e error
				if p.ct != nil {
					b, e = s.crypto.open(p.ct, inlineAAD(p.event, p.id))
				} else {
					b, e = s.blob(p.blob.String)
				}
				if e != nil {
					imageDiagnostics["unavailable_payload"]++
					continue
				}
				cfg, format, e := image.DecodeConfig(bytes.NewReader(b))
				if e != nil {
					imageDiagnostics["invalid_image"]++
				} else if !imageWithinBudget(cfg) {
					imageDiagnostics["pixel_limit"]++
				} else if !strings.HasSuffix(p.mime, format) {
					imageDiagnostics["mime_mismatch"]++
				} else {
					imageDiagnostics["valid_image"]++
				}
				if int64(len(b)) != p.size {
					imageDiagnostics["size_mismatch"]++
				}
				clear(b)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("source=%d recoverable=%d unavailable=%d missing_parts=%d", count, good, bad, missing)
	t.Logf("unavailable_by_kind=%v missing_parts_by_kind=%v", unavailableKinds, missingKinds)
	t.Logf("images_with_missing_parts=%v", imageDiagnostics)
	t.Logf("unavailable_file_records=%v", fileDiagnostics)
	if good+bad != count || good == 0 {
		t.Fatal("source verification did not cover its entries")
	}
	if target := os.Getenv("UNIC_V1_TEST_TARGET"); target != "" {
		report, err := s.Run(context.Background(), RunOptions{Target: target})
		if err != nil || report.VerificationFailed != 0 || report.Verified != good {
			t.Fatalf("repeat migration: %+v error=%v", report, err)
		}
		t.Logf("migration: imported=%d skipped=%d verified=%d unavailable=%d missing_parts=%d", report.Imported, report.Skipped, report.Verified, report.Failed, report.MissingParts)
		report, err = s.Run(context.Background(), RunOptions{Target: target})
		if err != nil || report.Imported != 0 || report.VerificationFailed != 0 || report.Verified != good {
			t.Fatalf("repeat migration: %+v error=%v", report, err)
		}
		t.Logf("repeat migration: skipped=%d verified=%d unavailable=%d", report.Skipped, report.Verified, report.Failed)
	}
}
