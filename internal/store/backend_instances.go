package store

import "database/sql"

type BackendInstance struct {
	ID             string
	Implementation string
	ConfigHash     string
}

// BindBackendInstances commits the configuration and authorization changes
// for every instance of a live replacement together.
func (s *Store) BindBackendInstances(bindings []BackendInstance) error {
	if len(bindings) == 0 {
		return nil
	}
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()

	for _, binding := range bindings {
		if err := bindBackendInstance(tx, binding); err != nil {
			return err
		}
	}
	return tx.Commit()
}

func bindBackendInstance(tx *sql.Tx, binding BackendInstance) error {
	id, implementation, configHash := binding.ID, binding.Implementation, binding.ConfigHash
	read := func(key string) (string, error) {
		var value string
		err := tx.QueryRow("SELECT v FROM meta WHERE k = ?", key).Scan(&value)
		if err == sql.ErrNoRows {
			return "", nil
		}
		return value, err
	}
	prefix := "backend.instance." + id + "."
	prior, err := read(prefix + "implementation")
	if err != nil {
		return err
	}
	if prior != "" && prior != implementation {
		return ErrDeviceConflict
	}
	previousHash, err := read(prefix + "config")
	if err != nil {
		return err
	}
	if previousHash != "" && previousHash != configHash {
		if _, err = tx.Exec("DELETE FROM device_bindings WHERE instance_id = ?", id); err != nil {
			return err
		}
		if _, err = tx.Exec("DELETE FROM channels WHERE instance_id = ?", id); err != nil {
			return err
		}
	}
	for key, value := range map[string]string{"implementation": implementation, "config": configHash} {
		if _, err = tx.Exec("INSERT INTO meta(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", prefix+key, value); err != nil {
			return err
		}
	}
	return nil
}
