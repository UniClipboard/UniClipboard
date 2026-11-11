package engine

import (
	"strings"
	"time"

	"github.com/uniclipboard/unic/plugin"
)

// cleanupHistoryLocked applies only the unified age/count policy. The same
// mutex protects tag changes, policy changes, and all record deletions.
func (e *Engine) cleanupHistoryLocked(keep int64) (ids []string) {
	h := e.hist
	p := h.cleanup
	if !p.AutoCleanup || e.opts.MigrationMode {
		return nil
	}
	n, err := e.st.CleanupHistoryCount(p.ExcludedTags)
	if err != nil {
		return nil
	}
	candidates, err := e.st.HistoryCleanupCandidates(p.ExcludedTags, keep, 256, "")
	if err != nil {
		return nil
	}
	cutoff := int64(0)
	if p.RetentionDays > 0 {
		cutoff = historyNow().Add(-time.Duration(p.RetentionDays) * 24 * time.Hour).UnixMilli()
	}
	for _, it := range candidates {
		if (cutoff <= 0 || it.CreatedMs > cutoff) && (p.MaxItems <= 0 || n <= p.MaxItems) {
			break
		}
		blobs, _, err := e.st.DeleteHistory(it.ID, 0)
		if err != nil {
			break
		}
		h.dropUnreferenced(e.st, blobs)
		ids = append(ids, it.ID)
		n--
	}
	h.refresh(e.st)
	e.reapCachedFiles()
	return ids
}

func (e *Engine) cleanupHistory() {
	if e.hist == nil || e.opts.MigrationMode {
		return
	}
	e.hist.mu.Lock()
	ids := e.cleanupHistoryLocked(0)
	e.hist.mu.Unlock()
	if len(ids) > 0 {
		e.historyChanged(ids...)
		e.notifyObservers(plugin.EventDeleted, ids...)
	}
}

// reserveCachedFiles applies the quota without a payload-only eviction path.
// Held files stay charged even after their record is removed.
func (e *Engine) reserveCachedFiles(n int64) bool {
	e.hist.mu.Lock()
	defer e.hist.mu.Unlock()
	e.mu.Lock()
	policy := e.settings.Value.FileCache
	e.mu.Unlock()
	if policy.MaxBytes == 0 {
		policy = DefaultSettings().FileCache
	}
	if n > policy.MaxBytes {
		return false
	}
	e.reapCachedFiles()
	if e.cache.reserve(n, policy) {
		return true
	}
	if !e.hist.cleanup.AutoCleanup || e.opts.MigrationMode {
		return false
	}
	var ids []string
	defer func() {
		if len(ids) > 0 {
			e.historyChanged(ids...)
			e.notifyObservers(plugin.EventDeleted, ids...)
		}
	}()
	candidates, err := e.st.HistoryCleanupCandidates(e.hist.cleanup.ExcludedTags, 0, 256, "cache")
	if err != nil {
		return false
	}
	for _, it := range candidates {
		full, err := e.st.GetHistory(it.ID, historyNow().UnixMilli(), false)
		if err != nil {
			continue
		}
		frees := false
		for _, p := range full.Parts {
			if strings.HasPrefix(p.Ref, "cache:") && e.cache.reclaimable(strings.TrimPrefix(p.Ref, "cache:"), e.hist.cleanup.ExcludedTags) {
				frees = true
				break
			}
		}
		if !frees {
			continue
		}
		blobs, _, err := e.st.DeleteHistory(it.ID, 0)
		if err != nil {
			return false
		}
		e.hist.dropUnreferenced(e.st, blobs)
		e.hist.refresh(e.st)
		ids = append(ids, it.ID)
		e.reapCachedFiles()
		if e.cache.reserve(n, policy) {
			return true
		}
	}
	return false
}

func (e *Engine) reapCachedFiles() {
	if e.cache != nil {
		e.cache.reap()
	}
}
