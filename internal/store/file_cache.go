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

// RetireUnreferencedCachedFile records pending physical deletion only when
// no retained history references the payload. It never invalidates history.
func (s *Store) RetireUnreferencedCachedFile(id string) (bool, error) {
	result, err := s.db.Exec(`UPDATE file_cache SET available=0 WHERE id=? AND NOT EXISTS (SELECT 1 FROM clip_item_parts WHERE ref=?)`, id, "cache:"+id)
	if err != nil {
		return false, err
	}
	n, err := result.RowsAffected()
	return n > 0, err
}
func (s *Store) ForgetCachedFile(id string) error {
	_, err := s.db.Exec(`DELETE FROM file_cache WHERE id=?`, id)
	return err
}

func (s *Store) PublishCachedFile(id string, completedMs int64) error {
	_, err := s.db.Exec(`UPDATE file_cache SET available=1, completed_ms=? WHERE id=?`, completedMs, id)
	return err
}
