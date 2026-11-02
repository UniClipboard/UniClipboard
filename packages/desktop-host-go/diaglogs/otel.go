package diaglogs

import (
	"archive/zip"
	"bufio"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
)

//go:embed import/collector.yaml
var collectorConfig []byte

//go:embed import/README.md
var importReadme []byte

//go:embed import/import.py
var importScript []byte

//go:embed log-attributes.json
var logPolicy []byte

type attributePolicy struct {
	Top            []string `json:"top"`
	Fields         []string `json:"fields"`
	Vocabulary     []string `json:"vocabulary"`
	StatusCounters []string `json:"statusCounters"`
}

func diagnosticSelection(root, signal string) ([]string, []any) {
	paths, issues := []string{}, []any{}
	directory := filepath.Join(root, "diagnostics", "engine")
	runs, err := os.ReadDir(directory)
	if err != nil && !os.IsNotExist(err) {
		issues = append(issues, map[string]any{"name": "diagnostics/engine", "state": "unreadable"})
	}
	for _, run := range runs {
		if !run.IsDir() || !uuidText(run.Name()) {
			continue
		}
		label := "diagnostics/engine/" + run.Name()
		dir := filepath.Join(directory, run.Name())
		entries, err := os.ReadDir(dir)
		if err != nil {
			state := "unreadable"
			if os.IsNotExist(err) {
				state = "missing"
			}
			issues = append(issues, map[string]any{"name": label, "state": state})
			continue
		}
		for _, entry := range entries {
			name := entry.Name()
			rest, ok := strings.CutPrefix(name, signal+".")
			if !ok {
				continue
			}
			rest, ok = strings.CutSuffix(rest, ".jsonl")
			if !ok {
				continue
			}
			parts := strings.Split(rest, ".")
			if len(parts) == 2 && len(parts[0]) == 8 && len(parts[1]) >= 10 && len(parts[1]) <= 20 && digits(parts[0]) && digits(parts[1]) && entry.Type().IsRegular() {
				paths = append(paths, filepath.Join(dir, name))
			}
		}
	}
	if signal == "logs" {
		entries, _ := os.ReadDir(root)
		for _, entry := range entries {
			if roleOf(entry.Name()) == "engine" && entry.Type().IsRegular() {
				paths = append(paths, filepath.Join(root, entry.Name()))
			}
		}
	}
	sort.Strings(paths)
	return paths, issues
}
func diagnosticFiles(root, signal string) []string {
	paths, _ := diagnosticSelection(root, signal)
	return paths
}
func digits(v string) bool {
	for _, b := range v {
		if b < '0' || b > '9' {
			return false
		}
	}
	return len(v) > 0
}
func uuidText(v string) bool {
	if len(v) != 36 {
		return false
	}
	for i, b := range v {
		if i == 8 || i == 13 || i == 18 || i == 23 {
			if b != '-' {
				return false
			}
		} else if !strings.ContainsRune("0123456789abcdefABCDEF", b) {
			return false
		}
	}
	return true
}
func hasDiagnosticFiles(root string) bool {
	for _, s := range []string{"metrics", "logs", "traces"} {
		paths, issues := diagnosticSelection(root, s)
		if len(paths) > 0 || len(issues) > 0 {
			return true
		}
	}
	return false
}

func diagnosticVersion(value string) bool {
	if len(value) > 32 {
		return false
	}
	base, suffix, hasSuffix := strings.Cut(value, "-")
	parts := strings.Split(base, ".")
	number := func(v string) bool { return len(v) > 0 && len(v) <= 5 && digits(v) }
	if len(parts) != 3 {
		return false
	}
	for _, part := range parts {
		if !number(part) {
			return false
		}
	}
	if hasSuffix {
		parts = strings.Split(suffix, ".")
		if len(parts) > 2 || !slices.Contains([]string{"alpha", "beta", "rc"}, parts[0]) {
			return false
		}
		if len(parts) == 2 && !number(parts[1]) {
			return false
		}
	}
	return true
}
func approvedAttribute(k string, v any, top bool, p attributePolicy) bool {
	keys := p.Fields
	if top {
		keys = p.Top
	}
	if !slices.Contains(keys, k) {
		return false
	}
	switch value := v.(type) {
	case string:
		if top {
			switch k {
			case "run_id", "peer_ref", "candidate_set_ref", "connection_id", "path_ref", "device_ref", "address_record_ref", "capture_id":
				return uuidText(value)
			case "host_version", "engine_version":
				return diagnosticVersion(value)
			case "source_commit":
				return value == "unknown" || (len(value) == 40 && isHex(value))
			}
		}
		return slices.Contains(p.Vocabulary, value)
	case json.Number:
		_, err := strconv.ParseUint(string(value), 10, 64)
		return err == nil
	case bool, nil:
		return true
	}
	return false
}
func logRequest(record map[string]any, p attributePolicy) ([]byte, error) {
	version, _ := record["local_schema_version"].(json.Number)
	if version != "1" && version != "2" {
		return nil, errors.New("unknown local log schema")
	}
	timestamp, _ := record["timestamp"].(string)
	at, err := time.Parse(time.RFC3339Nano, timestamp)
	if err != nil || at.UnixNano() < 0 {
		return nil, errors.New("invalid local timestamp")
	}
	level, _ := record["level"].(string)
	if level != "TRACE" && level != "DEBUG" && level != "INFO" && level != "WARN" && level != "ERROR" {
		return nil, errors.New("invalid severity")
	}
	attrs := []any{}
	appendAttribute := func(k string, v any, top bool) {
		if !approvedAttribute(k, v, top, p) {
			return
		}
		value := map[string]any{}
		switch x := v.(type) {
		case string:
			value["stringValue"] = x
		case bool:
			value["boolValue"] = x
		case json.Number:
			value["intValue"] = string(x)
		}
		attrs = append(attrs, map[string]any{"key": k, "value": value})
	}
	if fields, ok := record["fields"].(map[string]any); ok {
		keys := make([]string, 0, len(fields))
		for k := range fields {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			appendAttribute(k, fields[k], false)
		}
	}
	for _, k := range p.Top {
		if v, ok := record[k]; ok {
			appendAttribute(k, v, true)
		}
	}
	observed := at
	if text, ok := record["observed_timestamp"].(string); ok {
		if stamp, err := time.Parse(time.RFC3339Nano, text); err == nil && stamp.UnixNano() >= 0 {
			observed = stamp
		}
	}
	log := map[string]any{"timeUnixNano": strconv.FormatInt(at.UnixNano(), 10), "observedTimeUnixNano": strconv.FormatInt(observed.UnixNano(), 10), "severityText": level, "attributes": attrs}
	for _, item := range []struct {
		key, out string
		size     int
	}{{"trace_id", "traceId", 32}, {"span_id", "spanId", 16}} {
		if v, ok := record[item.key].(string); ok && len(v) == item.size && isHex(v) {
			log[item.out] = v
		}
	}
	return json.Marshal(map[string]any{"resourceLogs": []any{map[string]any{"scopeLogs": []any{map[string]any{"logRecords": []any{log}}}}}})
}

func diagnosticRunFacts(directory string, p attributePolicy) map[string]any {
	read := func(name string) map[string]any {
		f, err := openRegular(filepath.Join(directory, name))
		if err != nil {
			return nil
		}
		defer f.Close()
		info, err := f.Stat()
		if err != nil || info.Size() > 8192 {
			return nil
		}
		decoder := json.NewDecoder(io.LimitReader(f, 8192))
		decoder.UseNumber()
		var v map[string]any
		if decoder.Decode(&v) != nil {
			return nil
		}
		return v
	}
	run, status := read("run.json"), read("status.json")
	facts := map[string]any{"runId": filepath.Base(directory), "metadataReadable": run != nil, "countsReadable": status != nil, "countsComplete": false}
	for _, key := range []string{"schemaVersion", "engineSourceCommit", "engineSourceState", "startedAt"} {
		value := run[key]
		safe := false
		if key == "schemaVersion" {
			safe = value == json.Number("1")
		} else if v, ok := value.(string); ok {
			switch key {
			case "engineSourceCommit":
				safe = v == "unknown" || (len(v) == 40 && isHex(v))
			case "engineSourceState":
				safe = v == "clean" || v == "modified" || v == "unknown"
			case "startedAt":
				_, err := time.Parse(time.RFC3339Nano, v)
				safe = err == nil
			}
		}
		if safe {
			facts[key] = value
		}
	}
	for _, key := range p.StatusCounters {
		if v, ok := status[key].(json.Number); ok {
			if _, err := strconv.ParseUint(string(v), 10, 64); err == nil {
				facts[key] = v
			}
		}
	}
	for _, key := range []string{"closed", "countsComplete"} {
		if v, ok := status[key].(bool); ok {
			facts[key] = v
		}
	}
	if v, ok := status["updatedAt"].(string); ok {
		if _, err := time.Parse(time.RFC3339Nano, v); err == nil {
			facts["updatedAt"] = v
		}
	}
	return facts
}

// collectOfflineOTel reads finite prefixes. It never reads keys or starts a daemon.
func collectOfflineOTel(root string, archive *zip.Writer, logLimits map[string]int64) (bool, error) {
	var policy attributePolicy
	if err := json.Unmarshal(logPolicy, &policy); err != nil {
		return false, err
	}
	outputs, sources := []any{}, []any{}
	anyRecords := false
	selectionFailed := false
	runs := []any{}
	directories := map[string]bool{}
	for _, signal := range []string{"metrics", "logs", "traces"} {
		name := "otel/" + signal + ".jsonl"
		out, err := archive.CreateHeader(&zip.FileHeader{Name: name, Method: zip.Deflate})
		if err != nil {
			return false, err
		}
		hash := sha256.New()
		writer := io.MultiWriter(out, hash)
		var written, requests int64
		paths, issues := diagnosticSelection(root, signal)
		selectionFailed = selectionFailed || len(issues) > 0
		sources = append(sources, issues...)
		for _, path := range paths {
			directory := filepath.Dir(path)
			if directory != root && !directories[directory] {
				directories[directory] = true
				runs = append(runs, diagnosticRunFacts(directory, policy))
			}
			label, _ := filepath.Rel(root, path)
			label = filepath.ToSlash(label)
			source := map[string]any{"name": label, "observedAt": time.Now().UTC().Format(time.RFC3339Nano)}
			input, err := openRegular(path)
			if err != nil {
				source["state"] = "unreadable"
				if os.IsNotExist(err) {
					source["state"] = "missing"
				}
				sources = append(sources, source)
				continue
			}
			info, err := input.Stat()
			if err != nil {
				input.Close()
				source["state"] = "unreadable"
				sources = append(sources, source)
				continue
			}
			cutoff := info.Size()
			if limit, ok := logLimits[path]; ok && signal == "logs" {
				cutoff = min(cutoff, limit)
			}
			reader := bufio.NewReaderSize(io.LimitReader(input, cutoff), 64*1024)
			var consumed, invalid, tail int64
			readFailed := false
			for {
				line, err := reader.ReadSlice('\n')
				consumed += int64(len(line))
				if err == bufio.ErrBufferFull {
					invalid++
					for err == bufio.ErrBufferFull {
						line, err = reader.ReadSlice('\n')
						consumed += int64(len(line))
					}
					if err != nil && err != io.EOF {
						readFailed = true
					}
					if err != nil {
						break
					}
					continue
				}
				if err != nil {
					if err == io.EOF {
						tail += int64(len(line))
					} else {
						readFailed = true
					}
					break
				}
				var record map[string]any
				decoder := json.NewDecoder(strings.NewReader(string(line)))
				decoder.UseNumber()
				var extra any
				if decoder.Decode(&record) != nil || decoder.Decode(&extra) != io.EOF {
					invalid++
					continue
				}
				output := line
				if signal == "logs" {
					output, err = logRequest(record, policy)
					if err != nil {
						invalid++
						continue
					}
					output = append(output, '\n')
				} else {
					key := "resourceMetrics"
					if signal == "traces" {
						key = "resourceSpans"
					}
					if len(record) != 1 || record[key] == nil {
						invalid++
						continue
					}
				}
				n, err := writer.Write(output)
				if err != nil {
					input.Close()
					return false, err
				}
				written += int64(n)
				requests++
			}
			input.Close()
			source["cutoffBytes"] = cutoff
			source["readBytes"] = consumed
			source["truncated"] = consumed < cutoff
			source["readFailed"] = readFailed
			source["invalidLines"] = invalid
			source["tailBytes"] = tail
			sources = append(sources, source)
		}
		anyRecords = anyRecords || requests > 0
		state := "missing"
		if requests > 0 {
			state = "partial"
		}
		outputs = append(outputs, map[string]any{"name": name, "bytes": written, "requests": requests, "sha256": hex.EncodeToString(hash.Sum(nil)), "state": state})
	}
	state := "unavailable"
	if anyRecords {
		state = "partial"
	}
	summary, err := json.MarshalIndent(map[string]any{"schemaVersion": 1, "format": "OTLP JSONL", "collectorVersion": "0.139.0", "liveFlush": false, "state": state, "reason": "file_prefixes_other_processes_not_flushed", "recording": nil, "countsComplete": false, "files": outputs, "sources": sources, "runs": runs}, "", "  ")
	if err != nil {
		return false, err
	}
	for _, item := range []struct {
		name    string
		content []byte
	}{{"otel/manifest.json", summary}, {"import/collector.yaml", collectorConfig}, {"import/README.md", importReadme}, {"import/import.py", importScript}} {
		out, err := archive.CreateHeader(&zip.FileHeader{Name: item.name, Method: zip.Deflate})
		if err != nil {
			return false, err
		}
		if _, err = out.Write(item.content); err != nil {
			return false, err
		}
	}
	return anyRecords || selectionFailed, nil
}
