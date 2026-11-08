package v1migration

import (
	"archive/zip"
	"bytes"
	"context"
	"database/sql"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"image"
	_ "image/jpeg"
	"image/png"
	"io"
	"io/fs"
	"mime"
	"net/url"
	"os"
	pathpkg "path"
	"path/filepath"
	"runtime"
	"strings"
	"unicode/utf8"

	"github.com/klauspost/compress/zstd"
	"github.com/zeebo/blake3"
	_ "golang.org/x/image/tiff"
	_ "golang.org/x/image/webp"
	_ "modernc.org/sqlite"
)

const maxPayload = 500 << 20

type Source struct {
	root   string
	crypto *cryptoState
	db     *sql.DB
	tx     *sql.Tx
	tags   map[string]string
}

type Record struct {
	ID                       string
	CreatedAtMs, UpdatedAtMs int64
	Tags                     []string
	Parts                    []Part
	// Unrecoverable parts are reported even when other parts can be imported.
	MissingParts int
}

type Part struct {
	MIME, Name string
	Data       []byte
	Path       string
	Size       int64
	Hash       string
}

func (r *Record) Close() {
	for _, p := range r.Parts {
		clear(p.Data)
	}
	r.Parts = nil
}

func Open(ctx context.Context, root, service string, password []byte, secrets SecretReader) (*Source, error) {
	s, err := unlock(root, service, password, secrets)
	if err != nil {
		return nil, err
	}
	u := url.URL{Scheme: "file", Path: filepath.ToSlash(s.database)}
	db, err := sql.Open("sqlite", u.String()+"?mode=ro&_pragma=query_only(1)&_pragma=busy_timeout(5000)")
	if err != nil {
		s.close()
		return nil, ErrFormat
	}
	db.SetMaxOpenConns(1)
	tx, err := db.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		_ = db.Close()
		s.close()
		return nil, ErrFormat
	}
	src := &Source{root: root, crypto: s, db: db, tx: tx, tags: map[string]string{}}
	if err = src.loadTags(); err != nil {
		_ = src.Close()
		return nil, err
	}
	return src, nil
}
func (s *Source) Close() error { s.crypto.close(); _ = s.tx.Rollback(); return s.db.Close() }
func (s *Source) ID() string   { return s.crypto.sourceID }
func (s *Source) Count() (int, error) {
	var n int
	if s.tx.QueryRow("SELECT count(*) FROM clipboard_entry WHERE deleted_at_ms IS NULL").Scan(&n) != nil {
		return 0, ErrFormat
	}
	return n, nil
}
func (s *Source) TagNames() []string {
	var names []string
	for _, name := range s.tags {
		names = append(names, name)
	}
	return names
}
func (s *Source) loadTags() error {
	var exists int
	if s.tx.QueryRow("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='history_tag'").Scan(&exists) != nil {
		return ErrFormat
	}
	if exists == 0 {
		return nil
	}
	rows, err := s.tx.Query("SELECT tag_id,payload_ct FROM history_tag")
	if err != nil {
		return ErrFormat
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var id string
		var ct []byte
		if rows.Scan(&id, &ct) != nil {
			return ErrFormat
		}
		plain, err := s.crypto.open(ct, []byte("uc:history_tag:v1|"+id))
		if err != nil {
			return err
		}
		var payload struct {
			Name string `json:"name"`
		}
		err = json.Unmarshal(plain, &payload)
		clear(plain)
		if err != nil || payload.Name == "" {
			return ErrFormat
		}
		s.tags[id] = payload.Name
	}
	if rows.Err() != nil {
		return ErrFormat
	}
	return nil
}

// Walk holds a consistent, read-only SQLite snapshot and keeps only one page
// of identifiers and one record's payloads in memory.
func (s *Source) Walk(ctx context.Context, fn func(*Record, error) error) error {
	for offset := 0; ; offset += 128 {
		rows, err := s.tx.QueryContext(ctx, `SELECT entry_id,created_at_ms,active_time_ms,content_category FROM clipboard_entry
WHERE deleted_at_ms IS NULL ORDER BY created_at_ms,entry_id LIMIT 128 OFFSET ?`, offset)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return ErrFormat
		}
		type entry struct {
			id, category     string
			created, updated int64
		}
		var entries []entry
		for rows.Next() {
			var e entry
			if rows.Scan(&e.id, &e.created, &e.updated, &e.category) != nil {
				_ = rows.Close()
				return ErrFormat
			}
			entries = append(entries, e)
		}
		rowErr := rows.Err()
		_ = rows.Close()
		if rowErr != nil {
			return ErrFormat
		}
		if len(entries) == 0 {
			return nil
		}
		for _, e := range entries {
			if err = ctx.Err(); err != nil {
				return err
			}
			r := &Record{ID: e.id, CreatedAtMs: e.created, UpdatedAtMs: max(e.created, e.updated)}
			err = s.readRecord(r, e.category)
			callbackErr := fn(r, err)
			r.Close()
			if callbackErr != nil {
				return callbackErr
			}
		}
	}
}
func (s *Source) readRecord(r *Record, category string) error {
	if len(s.tags) > 0 {
		var ct []byte
		err := s.tx.QueryRow("SELECT tags_ct FROM history_tag_assignment WHERE entry_id=?", r.ID).Scan(&ct)
		if err == nil {
			plain, e := s.crypto.open(ct, []byte("uc:history_tag_assignment:v1|"+r.ID))
			if e != nil {
				return e
			}
			var ids []string
			e = json.Unmarshal(plain, &ids)
			clear(plain)
			if e != nil {
				return ErrFormat
			}
			for _, id := range ids {
				name, ok := s.tags[id]
				if !ok {
					return ErrFormat
				}
				r.Tags = append(r.Tags, name)
			}
		} else if err != sql.ErrNoRows {
			return ErrFormat
		}
	}
	if category == "file" {
		if err := s.readFiles(r); err != nil {
			return err
		}
		if len(r.Parts) > 0 {
			return nil
		}
		// Some old captures have a URI representation but no file manifest.
	}
	rows, err := s.tx.Query(`SELECT r.id,r.event_id,r.mime_type,r.format_id,
CASE WHEN length(r.inline_data)<=68157440 THEN r.inline_data ELSE NULL END,r.blob_id,r.size_bytes
FROM clipboard_snapshot_representation r JOIN clipboard_entry e ON e.event_id=r.event_id
WHERE e.entry_id=? ORDER BY r.id`, r.ID)
	if err != nil {
		return ErrFormat
	}
	type rep struct {
		id, event, mime, format, blob string
		ct                            []byte
		size                          int64
	}
	var reps []rep
	var cipherBytes int64
	for rows.Next() {
		var p rep
		var m, b sql.NullString
		if rows.Scan(&p.id, &p.event, &m, &p.format, &p.ct, &b, &p.size) != nil {
			_ = rows.Close()
			return ErrFormat
		}
		p.mime, p.blob = m.String, b.String
		if canonicalMIME(p.mime, p.format) == "" {
			clear(p.ct)
			continue
		}
		cipherBytes += int64(len(p.ct))
		if cipherBytes > maxPayload || len(reps) >= 128 {
			_ = rows.Close()
			return ErrContent
		}
		reps = append(reps, p)
	}
	defer func() {
		for _, p := range reps {
			clear(p.ct)
		}
	}()
	rowErr := rows.Err()
	_ = rows.Close()
	if rowErr != nil {
		return ErrFormat
	}
	seen := map[string]bool{}
	var total int64
	for _, p := range reps {
		m := canonicalMIME(p.mime, p.format)
		if m == "" || seen[m] {
			continue
		}
		var data []byte
		var e error
		if p.ct != nil {
			data, e = s.crypto.open(p.ct, inlineAAD(p.event, p.id))
		} else if p.blob != "" {
			data, e = s.blob(p.blob)
		} else {
			e = ErrContent
		}
		if e != nil {
			r.MissingParts++
			continue
		}
		// v1's background worker can replace TIFF with PNG and update MIME
		// without updating the representation's original size. blob() already
		// authenticates the payload and verifies its own size and ContentHash.
		if p.ct != nil && int64(len(data)) != p.size {
			clear(data)
			r.MissingParts++
			continue
		}
		if m == "text/uri-list" {
			e = s.uriFiles(r, data)
			clear(data)
			if e != nil {
				r.MissingParts++
			}
			continue
		}
		if strings.HasPrefix(m, "image/") && m != "image/png" {
			data, e = asPNG(data)
			m = "image/png"
			if e != nil {
				r.MissingParts++
				continue
			}
		}
		if m == "image/png" {
			cfg, e := png.DecodeConfig(bytes.NewReader(data))
			if e != nil || !imageWithinBudget(cfg) {
				clear(data)
				r.MissingParts++
				continue
			}
		}
		if seen[m] {
			clear(data)
			continue
		}
		if len(data) == 0 || (strings.HasPrefix(m, "text/") && !utf8.Valid(data)) {
			clear(data)
			r.MissingParts++
			continue
		}
		total += int64(len(data))
		if total > maxPayload {
			clear(data)
			return ErrContent
		}
		seen[m] = true
		r.Parts = append(r.Parts, Part{MIME: m, Data: data, Size: int64(len(data))})
	}
	if len(r.Parts) == 0 {
		return ErrContent
	}
	return nil
}

func canonicalMIME(m, format string) string {
	base, _, _ := mime.ParseMediaType(m)
	switch base {
	case "text/plain", "text/html", "image/png", "image/jpeg", "image/tiff", "image/webp", "text/uri-list":
		return base
	case "file/uri-list":
		return "text/uri-list"
	}
	switch format {
	case "public.utf8-plain-text", "UTF8_STRING":
		return "text/plain"
	case "public.html", "HTML Format":
		return "text/html"
	case "public.png":
		return "image/png"
	case "public.tiff":
		return "image/tiff"
	}
	return ""
}

func asPNG(data []byte) ([]byte, error) {
	defer clear(data)
	config, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || !imageWithinBudget(config) {
		return nil, ErrContent
	}
	img, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return nil, ErrContent
	}
	var out bytes.Buffer
	if png.Encode(&out, img) != nil {
		return nil, ErrContent
	}
	return out.Bytes(), nil
}
func imageWithinBudget(config image.Config) bool {
	return config.Width > 0 && config.Height > 0 && int64(config.Width) <= (32<<20)/int64(config.Height)
}
func (s *Source) blob(id string) ([]byte, error) {
	if id == "" || filepath.Base(id) != id || strings.ContainsAny(id, `/\`) {
		return nil, ErrFormat
	}
	path := filepath.Join(s.crypto.blobRoot, id)
	if !s.crypto.v3 {
		var stored string
		if s.tx.QueryRow("SELECT storage_path FROM blob WHERE blob_id=?", id).Scan(&stored) != nil {
			return nil, ErrContent
		}
		path = stored
	}
	data, err := readBounded(path, maxPayload+1<<20)
	if err != nil {
		return nil, err
	}
	defer clear(data)
	var plain []byte
	if bytes.HasPrefix(data, []byte("UCBL")) && len(data) > 5 {
		var compressed []byte
		switch data[4] {
		case 3:
			compressed, err = s.crypto.open(data[5:], []byte("uc:blob:v3|"+id))
		case 1:
			if len(data) < 45 {
				return nil, ErrFormat
			}
			compressed, err = openRaw(s.crypto.master, data[5:29], data[29:], []byte("uc:blob:v2|"+id))
		case 2:
			if len(data) < 14 {
				return nil, ErrFormat
			}
			epoch := binary.LittleEndian.Uint64(data[5:13])
			n := int(data[13])
			if 14+n+40 > len(data) {
				return nil, ErrFormat
			}
			key, ok := s.crypto.keys[string(data[14:14+n])]
			if !ok || key.epoch != epoch {
				return nil, ErrContent
			}
			aad := keyedAAD(key.space, epoch, string(data[14:14+n]), []byte("uc:blob:v2|"+id))
			compressed, err = openRaw(key.key, data[14+n:38+n], data[38+n:], aad)
		default:
			return nil, ErrFormat
		}
		if err != nil {
			return nil, err
		}
		defer clear(compressed)
		decoder, e := zstd.NewReader(bytes.NewReader(compressed), zstd.WithDecoderConcurrency(1), zstd.WithDecoderMaxMemory(maxPayload))
		if e != nil {
			return nil, ErrContent
		}
		defer decoder.Close()
		plain, err = io.ReadAll(io.LimitReader(decoder, maxPayload+1))
		if err != nil || len(plain) > maxPayload {
			clear(plain)
			return nil, ErrContent
		}
	} else {
		plain, err = s.crypto.open(data, []byte("uc:blob:v1|"+id))
		if err != nil {
			return nil, err
		}
	}
	var expected string
	var size int64
	if s.tx.QueryRow("SELECT content_hash,size_bytes FROM blob WHERE blob_id=?", id).Scan(&expected, &size) != nil {
		clear(plain)
		return nil, ErrFormat
	}
	sum := blake3.Sum256(plain)
	if int64(len(plain)) != size || !matchesContentHash(expected, sum[:]) {
		clear(plain)
		return nil, ErrContent
	}
	return plain, nil
}
func keyedAAD(space string, epoch uint64, id string, business []byte) []byte {
	field := func(dst, b []byte) []byte { return append(binary.LittleEndian.AppendUint64(dst, uint64(len(b))), b...) }
	aad := field(nil, []byte("uniclipboard-key-epoch-aad-v1"))
	aad = field(aad, []byte("ucbl-v2"))
	aad = field(aad, []byte(space))
	aad = binary.LittleEndian.AppendUint64(aad, epoch)
	aad = field(aad, []byte(id))
	aad = field(aad, []byte("content"))
	return field(aad, business)
}

func (s *Source) readFiles(r *Record) error {
	rows, err := s.tx.Query(`SELECT line_index,kind,blob_id,size_bytes,content_hash,original_text_ct,relative_path_ct,root_name_ct,kind_tag,root_index
FROM entry_file_set WHERE entry_id=? ORDER BY line_index`, r.ID)
	if err != nil {
		return ErrFormat
	}
	type file struct {
		index                    int64
		kind, blob, hash, tag    string
		size                     int64
		sizeKnown                bool
		original, relative, root []byte
		rootIndex                int64
	}
	var files []file
	for rows.Next() {
		var f file
		var blob, hash, tag sql.NullString
		var size sql.NullInt64
		var rootIndex sql.NullInt64
		if rows.Scan(&f.index, &f.kind, &blob, &size, &hash, &f.original, &f.relative, &f.root, &tag, &rootIndex) != nil {
			_ = rows.Close()
			return ErrFormat
		}
		f.blob, f.hash, f.tag, f.size = blob.String, hash.String, tag.String, size.Int64
		f.sizeKnown = size.Valid
		f.rootIndex = rootIndex.Int64
		files = append(files, f)
		if len(files) > 10000 {
			_ = rows.Close()
			return ErrContent
		}
	}
	rowErr := rows.Err()
	_ = rows.Close()
	if rowErr != nil {
		return ErrFormat
	}
	var total int64
	type group struct {
		name    string
		members []archiveMember
	}
	groups := map[int64]*group{}
	var groupIDs []int64
	addMember := func(index int64, root string, m archiveMember) {
		g := groups[index]
		if g == nil {
			g = &group{name: root}
			groups[index] = g
			groupIDs = append(groupIDs, index)
		}
		g.members = append(g.members, m)
	}
	defer func() {
		for _, g := range groups {
			for _, m := range g.members {
				clear(m.part.Data)
			}
		}
	}()
	for _, f := range files {
		if f.kind != "file" {
			if f.kind == "excluded" {
				r.MissingParts++
			}
			continue
		}
		aad := fmt.Sprintf("uc:file_set_line:v1|%s|%d", r.ID, f.index)
		path, e := s.openFileColumn(f.original, aad+"|original-text")
		if e != nil {
			r.MissingParts++
			continue
		}
		name := filepath.Base(path)
		if u, e := url.Parse(path); e == nil && u.Scheme == "file" {
			path = localFilePath(u)
			name = filepath.Base(path)
		}
		var archiveRoot, archivePath string
		if f.root != nil && f.relative != nil {
			root, e1 := s.openFileColumn(f.root, aad+"|root-name")
			relative, e2 := s.openFileColumn(f.relative, aad+"|relative-path")
			if e1 != nil || e2 != nil {
				r.MissingParts++
				continue
			}
			if relative != "" || f.tag == "d" {
				if root == "" || strings.ContainsAny(root, `/\`) || root == "." || root == ".." ||
					relative != "" && (!fs.ValidPath(relative) || strings.Contains(relative, `\`)) {
					r.MissingParts++
					continue
				}
				archiveRoot = root
				archivePath = pathpkg.Join(root, relative)
				if f.tag == "d" {
					archivePath += "/"
				}
			} else if root != "" {
				name = root
			}
		}
		if f.tag == "d" {
			if archiveRoot == "" {
				r.MissingParts++
				continue
			}
			addMember(f.rootIndex, archiveRoot, archiveMember{name: archivePath, directory: true})
			continue
		}
		if name == "" || name == "." || name == string(filepath.Separator) {
			r.MissingParts++
			continue
		}
		part := Part{Name: name, MIME: "application/octet-stream"}
		if f.blob != "" {
			data, e := s.blob(f.blob)
			if e != nil {
				r.MissingParts++
				continue
			}
			total += int64(len(data))
			if total > maxPayload {
				clear(data)
				return ErrContent
			}
			part.Data, part.Size = data, int64(len(data))
		} else {
			local, e := externalFilePart(path, f.size, f.sizeKnown, f.hash)
			if e != nil {
				r.MissingParts++
				continue
			}
			part.Path, part.Size, part.Hash = local.Path, local.Size, local.Hash
		}
		if archiveRoot != "" {
			addMember(f.rootIndex, archiveRoot, archiveMember{name: archivePath, part: part})
		} else {
			r.Parts = append(r.Parts, part)
		}
	}
	for _, id := range groupIDs {
		g := groups[id]
		data, err := archiveDirectory(g.members)
		for _, m := range g.members {
			clear(m.part.Data)
		}
		if err != nil {
			r.MissingParts++
			continue
		}
		var retained int64
		for _, p := range r.Parts {
			retained += int64(len(p.Data))
		}
		if retained+int64(len(data)) > maxPayload {
			clear(data)
			return ErrContent
		}
		r.Parts = append(r.Parts, Part{Name: g.name + ".zip", MIME: "application/zip", Data: data, Size: int64(len(data))})
	}
	return nil
}
func externalFilePart(path string, expectedSize int64, sizeKnown bool, hash string) (Part, error) {
	st, err := os.Stat(path)
	if err != nil || !st.Mode().IsRegular() || sizeKnown && st.Size() != expectedSize {
		return Part{}, ErrContent
	}
	return Part{Path: path, Size: st.Size(), Hash: hash}, nil
}
func (s *Source) openFileColumn(ct []byte, aad string) (string, error) {
	var b []byte
	var err error
	if s.crypto.v3 {
		b, err = s.crypto.open(ct, []byte(aad))
	} else {
		if len(ct) < 45 || !bytes.HasPrefix(ct, []byte("UCFS\x01")) {
			return "", ErrFormat
		}
		key := derive(s.crypto.master, []byte("default"), []byte("uniclipboard-file-set/v1"))
		defer clear(key)
		b, err = openRaw(key, ct[5:29], ct[29:], []byte(aad))
	}
	if err != nil {
		return "", err
	}
	defer clear(b)
	if !utf8.Valid(b) {
		return "", ErrFormat
	}
	return string(b), nil
}
func (s *Source) uriFiles(r *Record, data []byte) error {
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		u, err := url.Parse(line)
		if err != nil || u.Scheme != "file" || u.Host != "" && u.Host != "localhost" {
			r.MissingParts++
			continue
		}
		path := localFilePath(u)
		st, err := os.Stat(path)
		if err != nil || !st.Mode().IsRegular() {
			r.MissingParts++
			continue
		}
		r.Parts = append(r.Parts, Part{Name: filepath.Base(path), Path: path, MIME: "application/octet-stream", Size: st.Size()})
	}
	return nil
}

func localFilePath(u *url.URL) string {
	p := u.Path
	if runtime.GOOS == "windows" && len(p) > 2 && p[0] == '/' && p[2] == ':' {
		p = p[1:]
	}
	return filepath.FromSlash(p)
}

type archiveMember struct {
	name      string
	directory bool
	part      Part
}
type boundedArchive struct{ bytes.Buffer }

func (b *boundedArchive) Write(p []byte) (int, error) {
	if len(p) > maxPayload-b.Len() {
		return 0, ErrContent
	}
	return b.Buffer.Write(p)
}
func archiveDirectory(members []archiveMember) ([]byte, error) {
	var b boundedArchive
	w := zip.NewWriter(&b)
	seen := map[string]bool{}
	for _, m := range members {
		if seen[m.name] {
			continue
		}
		seen[m.name] = true
		entry, err := w.CreateHeader(&zip.FileHeader{Name: m.name, Method: zip.Store})
		if err != nil {
			return nil, ErrContent
		}
		if m.directory {
			continue
		}
		reader, err := m.part.Open()
		if err != nil {
			return nil, err
		}
		n, err := io.Copy(entry, reader)
		_ = reader.Close()
		if err != nil || n != m.part.Size {
			return nil, ErrContent
		}
	}
	if w.Close() != nil {
		return nil, ErrContent
	}
	return b.Bytes(), nil
}
