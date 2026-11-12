package host

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/uniclipboard/unic/internal/localcontent"
)

type directoryStage struct{ Root, Name string }

// Journal only our exact unpublished staging directory. Never enumerate the
// custom destination or journal the published destination as a cleanup target.
func recordDirectoryStage(dataDir, root, name string, info os.FileInfo) (string, error) {
	dir := filepath.Join(dataDir, "directory-staging")
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	path := filepath.Join(dir, name+".json")
	data, _ := json.Marshal(directoryStage{Root: localcontent.NewDirectorySource(root, info).PersistentRef(), Name: name})
	f, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return "", err
	}
	_, err = f.Write(data)
	if err == nil {
		err = f.Sync()
	}
	if closeErr := f.Close(); err == nil {
		err = closeErr
	}
	return path, err
}
func recoverDirectoryStages(dataDir string) error {
	dir := filepath.Join(dataDir, "directory-staging")
	entries, err := os.ReadDir(dir)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return errors.New("directory staging recovery failed")
	}
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		path := filepath.Join(dir, entry.Name())
		data, err := os.ReadFile(path)
		if err != nil || len(data) > 65536 {
			return errors.New("directory staging journal unavailable")
		}
		var target directoryStage
		if json.Unmarshal(data, &target) != nil || !strings.HasPrefix(target.Name, ".unic-directory-stage-") || target.Name+".json" != entry.Name() || filepath.Base(target.Name) != target.Name {
			return errors.New("invalid directory staging journal")
		}
		source := localcontent.ResolveDirectory(target.Root)
		if source == nil {
			return errors.New("invalid directory staging root")
		}
		r, _, err := source.Open(context.Background())
		if err != nil {
			return errors.New("directory staging root changed")
		}
		_ = r.Close()
		rootPath, ok := localcontent.DirectoryPath(target.Root)
		if !ok {
			return errors.New("invalid directory staging root")
		}
		root, err := os.OpenRoot(rootPath)
		if err != nil {
			return errors.New("directory staging recovery failed")
		}
		err = root.RemoveAll(target.Name)
		_ = root.Close()
		if err != nil {
			return errors.New("directory staging cleanup failed")
		}
		if err := os.Remove(path); err != nil {
			return errors.New("directory staging recovery failed")
		}
	}
	return nil
}
