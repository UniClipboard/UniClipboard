package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"
)

var ErrSearchIncomplete = errors.New("store: search index incomplete")

const searchSchema = `
CREATE TABLE IF NOT EXISTS history_search_totals(id INTEGER PRIMARY KEY CHECK(id=1),reserved INTEGER NOT NULL);
INSERT OR IGNORE INTO history_search_totals VALUES(1,0);
CREATE TABLE IF NOT EXISTS history_search_state(item_seq INTEGER PRIMARY KEY REFERENCES clip_items(seq) ON DELETE CASCADE,ready INTEGER NOT NULL,reserved INTEGER NOT NULL);
CREATE TRIGGER IF NOT EXISTS history_search_reserve AFTER INSERT ON history_search_state BEGIN UPDATE history_search_totals SET reserved=reserved+new.reserved WHERE id=1; END;
CREATE TRIGGER IF NOT EXISTS history_search_release AFTER DELETE ON history_search_state BEGIN UPDATE history_search_totals SET reserved=reserved-old.reserved WHERE id=1; END;
CREATE TABLE IF NOT EXISTS history_search_docs(id INTEGER PRIMARY KEY,item_seq INTEGER NOT NULL REFERENCES clip_items(seq) ON DELETE CASCADE,body TEXT NOT NULL,folded TEXT NOT NULL,extension TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS history_search_item ON history_search_docs(item_seq);
CREATE INDEX IF NOT EXISTS history_search_extension ON history_search_docs(extension,item_seq);
CREATE VIRTUAL TABLE IF NOT EXISTS history_search_fts USING fts5(folded,content='history_search_docs',content_rowid='id',tokenize='trigram case_sensitive 1');
CREATE TRIGGER IF NOT EXISTS history_search_ai AFTER INSERT ON history_search_docs BEGIN INSERT INTO history_search_fts(rowid,folded) VALUES(new.id,new.folded); END;
CREATE TRIGGER IF NOT EXISTS history_search_ad AFTER DELETE ON history_search_docs BEGIN INSERT INTO history_search_fts(history_search_fts,rowid,folded) VALUES('delete',old.id,old.folded); END;
`

type SearchDocument struct{ Text, Folded, Extension string }
type SearchSource struct{ Kind, DeviceID string }
type SearchFilter struct {
	HistoryFilter
	Terms, Types, Extensions []string
	TagAny                   bool
	Sources                  []SearchSource
}
type SearchRead struct {
	Filter                         SearchFilter
	Upper, After, AfterTime, NowMs int64
	Limit                          int
}
type SearchHit struct {
	Item    HistoryItem
	Preview string
}
type SearchCandidate struct{ Key, Value, Label, Kind, DeviceID string }

func openSearch(db *sql.DB, path string) (*sql.DB, error) {
	if _, err := db.Exec(searchSchema); err != nil {
		return nil, err
	}
	r, err := sql.Open("sqlite", "file:"+path+"?mode=ro&_pragma=busy_timeout(1000)&_pragma=query_only(1)&_pragma=foreign_keys(1)")
	if err != nil {
		return nil, err
	}
	r.SetMaxOpenConns(1)
	r.SetMaxIdleConns(1)
	if err = r.Ping(); err != nil {
		_ = r.Close()
		return nil, err
	}
	return r, nil
}
func insertSearch(tx *sql.Tx, seq int64, docs []SearchDocument, ready bool, budget int64) error {
	var reserved int64
	for _, d := range docs {
		reserved += int64(8*(len(d.Text)+len(d.Folded)) + 256)
	}
	var used int64
	if err := tx.QueryRow(`SELECT reserved FROM history_search_totals WHERE id=1`).Scan(&used); err != nil {
		return err
	}
	if budget <= 0 {
		budget = 2 << 30
	}
	if !ready || reserved > budget-used {
		ready = false
		reserved = 0
		docs = nil
	}
	if _, err := tx.Exec(`INSERT INTO history_search_state(item_seq,ready,reserved) VALUES(?,?,?)`, seq, ready, reserved); err != nil {
		return err
	}
	for _, d := range docs {
		if _, err := tx.Exec(`INSERT INTO history_search_docs(item_seq,body,folded,extension) VALUES(?,?,?,?)`, seq, d.Text, d.Folded, d.Extension); err != nil {
			return err
		}
	}
	return nil
}
func (s *Store) RepairSearch(seq int64, docs []SearchDocument, ready bool, budget int64) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	if _, err = tx.Exec(`DELETE FROM history_search_docs WHERE item_seq=?`, seq); err != nil {
		return err
	}
	if _, err = tx.Exec(`DELETE FROM history_search_state WHERE item_seq=?`, seq); err != nil {
		return err
	}
	if err = insertSearch(tx, seq, docs, ready, budget); err != nil {
		return err
	}
	return tx.Commit()
}
func (s *Store) SearchUnready(after int64) ([]HistoryItem, error) {
	rows, err := s.db.Query(`SELECT `+itemCols+` FROM clip_items WHERE seq>? AND NOT EXISTS(SELECT 1 FROM history_search_state WHERE item_seq=seq AND ready=1) ORDER BY seq LIMIT 32`, after)
	if err != nil {
		return nil, err
	}
	var items []HistoryItem
	for rows.Next() {
		it, e := scanItem(rows)
		if e != nil {
			_ = rows.Close()
			return nil, e
		}
		items = append(items, it)
	}
	err = rows.Err()
	_ = rows.Close()
	if err != nil {
		return nil, err
	}
	for i := range items {
		if err = fill(s.db, &items[i], true); err != nil {
			return nil, err
		}
	}
	return items, nil
}

// historyPredicate is shared by metadata listing and explicit search.
func historyPredicate(q HistoryQuery) ([]string, []any) {
	var where []string
	var args []any
	add := func(cond string, a ...any) { where = append(where, cond); args = append(args, a...) }
	if q.Upper > 0 {
		add(`seq<=?`, q.Upper)
	}
	if q.After > 0 {
		add(`(`+orderExpr+`<? OR (`+orderExpr+`=? AND seq<?))`, q.AfterTime, q.AfterTime, q.After)
	}
	if q.NowMs > 0 {
		add(`(expires_ms=0 OR expires_ms>?)`, q.NowMs)
	}
	f := q.Filter
	in := func(col string, vs []string) {
		if len(vs) > 0 {
			add(col+` IN (`+placeholders(len(vs))+`)`, anys(vs)...)
		}
	}
	in("kind", f.Kinds)
	in("origin_kind", f.OriginKinds)
	if f.DeviceID != "" {
		add(`origin_kind='remote_device' AND device_id=?`, f.DeviceID)
	}
	if f.FromMs > 0 {
		add(`created_ms>=?`, f.FromMs)
	}
	if f.BeforeMs > 0 {
		add(`created_ms<?`, f.BeforeMs)
	}
	for _, t := range f.Tags {
		add(`EXISTS(SELECT 1 FROM clip_tags WHERE item_seq=clip_items.seq AND tag=?)`, t)
	}
	return where, args
}
func placeholders(n int) string { return strings.TrimSuffix(strings.Repeat("?,", n), ",") }
func searchPredicate(q SearchRead, terms bool) (string, []any) {
	f := q.Filter
	base := f.HistoryFilter
	if f.TagAny {
		base.Tags = nil
	}
	where, args := historyPredicate(HistoryQuery{Filter: base, Upper: q.Upper, After: q.After, AfterTime: q.AfterTime, NowMs: q.NowMs})
	add := func(cond string, a ...any) { where = append(where, cond); args = append(args, a...) }
	if f.TagAny && len(f.Tags) > 0 {
		add(`EXISTS(SELECT 1 FROM clip_tags WHERE item_seq=clip_items.seq AND tag IN (`+placeholders(len(f.Tags))+`))`, anys(f.Tags)...)
	}
	if len(f.Types) > 0 {
		var parts []string
		for _, t := range f.Types {
			switch t {
			case "richtext":
				parts = append(parts, `(kind='text' AND EXISTS(SELECT 1 FROM clip_item_parts WHERE item_seq=seq AND mime='text/html'))`)
			default:
				parts = append(parts, `kind=?`)
				args = append(args, t)
			}
		}
		where = append(where, "("+strings.Join(parts, " OR ")+")")
	}
	if len(f.Sources) > 0 {
		var parts []string
		for _, src := range f.Sources {
			switch src.Kind {
			case "local":
				parts = append(parts, `origin_kind IN ('local_system','explicit','action')`)
			case "device":
				parts = append(parts, `(origin_kind='remote_device' AND device_id=?)`)
				args = append(args, src.DeviceID)
			default:
				parts = append(parts, `origin_kind=?`)
				args = append(args, src.Kind)
			}
		}
		where = append(where, "("+strings.Join(parts, " OR ")+")")
	}
	if terms {
		if len(f.Extensions) > 0 {
			add(`EXISTS(SELECT 1 FROM history_search_docs WHERE item_seq=clip_items.seq AND extension IN (`+placeholders(len(f.Extensions))+`))`, anys(f.Extensions)...)
		}
		for _, term := range f.Terms {
			predicate := `item_seq=clip_items.seq AND instr(folded,?)>0`
			a := []any{term}
			if utf8.RuneCountInString(term) >= 3 {
				predicate += ` AND id IN (SELECT rowid FROM history_search_fts WHERE history_search_fts MATCH ?)`
				a = append(a, `"`+strings.ReplaceAll(term, `"`, `""`)+`"`)
			}
			add(`EXISTS(SELECT 1 FROM history_search_docs WHERE `+predicate+`)`, a...)
		}
	}
	if len(where) == 0 {
		return "1", args
	}
	return strings.Join(where, " AND "), args
}
func checkSearchReady(ctx context.Context, tx *sql.Tx, q SearchRead) (bool, error) {
	where, args := searchPredicate(q, false)
	var missing int
	err := tx.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM clip_items WHERE `+where+` AND NOT EXISTS(SELECT 1 FROM history_search_state WHERE item_seq=seq AND ready=1))`, args...).Scan(&missing)
	if err != nil {
		return false, err
	}
	if missing != 0 && (len(q.Filter.Terms) > 0 || len(q.Filter.Extensions) > 0) {
		return false, ErrSearchIncomplete
	}
	return missing == 0, nil
}
func (s *Store) SearchHistory(ctx context.Context, q SearchRead) (hits []SearchHit, rev uint64, newest int64, complete bool, err error) {
	tx, e := s.search.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if e != nil {
		err = e
		return
	}
	defer func() { _ = tx.Rollback() }()
	if err = tx.QueryRowContext(ctx, `SELECT revision FROM history_state WHERE id=1`).Scan(&rev); err != nil {
		return
	}
	if err = tx.QueryRowContext(ctx, `SELECT COALESCE(MAX(seq),0) FROM clip_items`).Scan(&newest); err != nil {
		return
	}
	if complete, err = checkSearchReady(ctx, tx, q); err != nil {
		return
	}
	where, args := searchPredicate(q, true)
	args = append(args, q.Limit)
	rows, e := tx.QueryContext(ctx, `SELECT `+itemCols+` FROM clip_items WHERE `+where+` ORDER BY `+orderExpr+` DESC,seq DESC LIMIT ?`, args...)
	if e != nil {
		err = e
		return
	}
	for rows.Next() {
		it, e := scanItem(rows)
		if e != nil {
			_ = rows.Close()
			err = e
			return
		}
		hits = append(hits, SearchHit{Item: it})
	}
	err = rows.Err()
	_ = rows.Close()
	if err != nil {
		return
	}
	for i := range hits {
		if err = ctx.Err(); err != nil {
			return
		}
		if err = fill(tx, &hits[i].Item, false); err != nil {
			return
		}
		// Find a matching document and center a bounded preview around the first term.
		term := ""
		if len(q.Filter.Terms) > 0 {
			term = q.Filter.Terms[0]
		}
		e = tx.QueryRowContext(ctx, `SELECT substr(body,max(instr(folded,?)-24,1),512) FROM history_search_docs WHERE item_seq=? ORDER BY (instr(folded,?)>0) DESC,id LIMIT 1`, term, hits[i].Item.Seq, term).Scan(&hits[i].Preview)
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			err = e
			return
		}
	}
	return
}
func (s *Store) CountHistorySearches(ctx context.Context, filters []SearchFilter, now int64) (counts []uint64, rev uint64, err error) {
	tx, e := s.search.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if e != nil {
		return nil, 0, e
	}
	defer func() { _ = tx.Rollback() }()
	if err = tx.QueryRowContext(ctx, `SELECT revision FROM history_state WHERE id=1`).Scan(&rev); err != nil {
		return
	}
	cache := map[string]uint64{}
	for _, f := range filters {
		q := SearchRead{Filter: f, NowMs: now}
		if _, err = checkSearchReady(ctx, tx, q); err != nil {
			return nil, 0, err
		}
		where, args := searchPredicate(q, true)
		keyBytes, _ := json.Marshal(f)
		key := string(keyBytes)
		n, ok := cache[key]
		if !ok {
			if err = tx.QueryRowContext(ctx, `SELECT COUNT(*) FROM clip_items WHERE `+where, args...).Scan(&n); err != nil {
				return nil, 0, err
			}
			cache[key] = n
		}
		counts = append(counts, n)
	}
	return
}
func (s *Store) SearchCandidates(ctx context.Context, field, prefix, after string, limit int, now int64) (out []SearchCandidate, rev uint64, err error) {
	tx, e := s.search.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if e != nil {
		return nil, 0, e
	}
	defer func() { _ = tx.Rollback() }()
	if err = tx.QueryRowContext(ctx, `SELECT revision FROM history_state WHERE id=1`).Scan(&rev); err != nil {
		return
	}
	var query string
	var args []any
	switch field {
	case "tag":
		query = `SELECT name,name,name,'','' FROM (SELECT name FROM history_labels UNION SELECT tag AS name FROM clip_tags) ORDER BY CAST(name AS BLOB)`
	case "extension":
		if _, err = checkSearchReady(ctx, tx, SearchRead{NowMs: now, Filter: SearchFilter{Terms: []string{"ready"}}}); err != nil {
			return
		}
		query = `SELECT DISTINCT d.extension,d.extension,d.extension,'','' FROM history_search_docs d JOIN clip_items i ON i.seq=d.item_seq WHERE d.extension<>'' AND (i.expires_ms=0 OR i.expires_ms>?) ORDER BY d.extension`
		args = []any{now}
	case "source":
		query = `SELECT 'local','local','本机','local','' UNION SELECT DISTINCT CASE WHEN i.origin_kind='remote_device' THEN 'device:'||i.device_id ELSE i.origin_kind END,CASE WHEN i.origin_kind='remote_device' THEN i.device_id ELSE i.origin_kind END,CASE WHEN i.origin_kind='remote_device' THEN COALESCE(NULLIF(d.name,''),NULLIF(d.reported_name,''),i.device_id) WHEN i.origin_kind='migration' THEN '从旧版导入' WHEN i.origin_kind='external' THEN '外部来源' ELSE '本机' END,CASE WHEN i.origin_kind='remote_device' THEN 'device' WHEN i.origin_kind IN ('local_system','explicit','action') THEN 'local' ELSE i.origin_kind END,i.device_id FROM clip_items i LEFT JOIN devices d ON d.id=i.device_id WHERE i.origin_kind IN ('remote_device','external','migration') AND (i.expires_ms=0 OR i.expires_ms>?) ORDER BY 1`
		args = []any{now}
	default:
		return nil, 0, errors.New("invalid catalogue field")
	}
	rows, e := tx.QueryContext(ctx, query, args...)
	if e != nil {
		return nil, 0, e
	}
	defer func() { _ = rows.Close() }()
	for rows.Next() {
		var v SearchCandidate
		if err = rows.Scan(&v.Key, &v.Value, &v.Label, &v.Kind, &v.DeviceID); err != nil {
			return
		}
		if v.Key <= after || (!strings.Contains(strings.ToLower(v.Value), prefix) && !strings.Contains(strings.ToLower(v.Label), prefix)) {
			continue
		}
		out = append(out, v)
		if len(out) >= limit {
			break
		}
	}
	err = rows.Err()
	return
}
