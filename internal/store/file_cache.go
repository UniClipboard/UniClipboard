package store

// CachedFile describes a file whose deletion authority is confined to the default cache.
type CachedFile struct {
	ID, Name          string
	Size, CompletedMs int64
	Available         bool
}

func (s *Store) CachedFiles() ([]CachedFile, error) {
	rows, err := s.db.Query(`SELECT id,name,size,completed_ms,available FROM file_cache ORDER BY completed_ms,id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []CachedFile
	for rows.Next() {
		var f CachedFile
		if err := rows.Scan(&f.ID, &f.Name, &f.Size, &f.CompletedMs, &f.Available); err != nil {
			return nil, err
		}
		out = append(out, f)
	}
	return out, rows.Err()
}
func (s *Store) PutCachedFile(f CachedFile) error {
	_, err := s.db.Exec(`INSERT INTO file_cache(id,name,size,completed_ms,available) VALUES(?,?,?,?,?)`, f.ID, f.Name, f.Size, f.CompletedMs, f.Available)
	return err
}
func (s *Store) MarkCachedFileUnavailable(id string) ([]string, error) {
	tx, err := s.db.Begin()
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.Query(`SELECT DISTINCT i.id FROM clip_items i JOIN clip_item_parts p ON p.item_seq=i.seq WHERE p.ref=?`, "cache:"+id)
	if err != nil {
		return nil, err
	}
	var ids []string
	for rows.Next() {
		var value string
		if err = rows.Scan(&value); err != nil {
			_ = rows.Close()
			return nil, err
		}
		ids = append(ids, value)
	}
	if err = rows.Close(); err != nil {
		return nil, err
	}
	if _, err = tx.Exec(`UPDATE file_cache SET available=0 WHERE id=?`, id); err != nil {
		return nil, err
	}
	if len(ids) > 0 {
		if _, err = tx.Exec(`UPDATE clip_items SET revision=revision+1 WHERE seq IN (SELECT item_seq FROM clip_item_parts WHERE ref=?)`, "cache:"+id); err != nil {
			return nil, err
		}
		if _, err = bump(tx); err != nil {
			return nil, err
		}
	}
	return ids, tx.Commit()
}
func (s *Store) ForgetCachedFile(id string) error {
	_, err := s.db.Exec(`DELETE FROM file_cache WHERE id=?`, id)
	return err
}

func (s *Store) PublishCachedFile(id string, completedMs int64) error {
	_, err := s.db.Exec(`UPDATE file_cache SET available=1, completed_ms=? WHERE id=?`, completedMs, id)
	return err
}
