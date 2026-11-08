package store

import "database/sql"

func (s *Store) LookupHistoryImport(source, record string) (string, bool, error) {
	var id string
	err := s.db.QueryRow(`SELECT history_id FROM history_imports WHERE source=? AND record_id=?`, source, record).Scan(&id)
	if err == sql.ErrNoRows {
		return "", false, nil
	}
	return id, err == nil, err
}

func (s *Store) HistoryImportLimits() (items int, bytes, stored int64, err error) {
	err = s.db.QueryRow(`SELECT max_items,max_bytes,max_stored_bytes FROM history_import_limits WHERE id=1`).Scan(&items, &bytes, &stored)
	if err == sql.ErrNoRows {
		err = nil
	}
	return
}

func (s *Store) ImportHistoryTags(names []string) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	for _, name := range names {
		if _, err = tx.Exec(`INSERT OR IGNORE INTO history_labels(name) VALUES(?)`, name); err != nil {
			return err
		}
	}
	return tx.Commit()
}

func (s *Store) ListHistoryTags() ([]string, error) {
	rows, err := s.db.Query(`SELECT name FROM history_labels UNION SELECT tag FROM clip_tags ORDER BY 1`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	names := []string{}
	for rows.Next() {
		var name string
		if err = rows.Scan(&name); err != nil {
			return nil, err
		}
		names = append(names, name)
	}
	return names, rows.Err()
}
