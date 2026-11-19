package main

import (
	"encoding/json"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

const protocolVersion = 1

type request struct {
	ID     uint64          `json:"id"`
	Method string          `json:"method"`
	Value  json.RawMessage `json:"value,omitempty"`
}
type response struct {
	ID    uint64          `json:"id"`
	Value json.RawMessage `json:"value,omitempty"`
	Error string          `json:"error,omitempty"`
}
type hello struct {
	Socket    string `json:"socket"`
	Protocol  int    `json:"protocol"`
	OS        string `json:"os"`
	Arch      string `json:"arch"`
	Machine   string `json:"machine"`
	WorkerSHA string `json:"worker_sha256"`
	Root      string `json:"root"`
	FreeBytes uint64 `json:"free_bytes"`
}
type installConfig struct {
	Name    string            `json:"name"`
	Hashes  map[string]string `json:"hashes"`
	Plugins map[string][]byte `json:"plugins"`
}
type expectedPart struct {
	Kind string `json:"kind"`
	MIME string `json:"mime"`
	Name string `json:"name,omitempty"`
	Size int64  `json:"size"`
	SHA  string `json:"sha256"`
}
type fixture struct {
	ID    string         `json:"id"`
	Kind  string         `json:"kind"`
	Parts []expectedPart `json:"parts"`
	Total int64          `json:"total_bytes"`
}
type prepareRequest struct {
	Payload payload `json:"payload"`
	Seed    string  `json:"seed"`
}
type submitRequest struct {
	Fixture string `json:"fixture"`
	Key     string `json:"key"`
}
type submitResult struct {
	Result     engine.SubmissionResult `json:"result"`
	StartedNS  int64                   `json:"started_ns"`
	FinishedNS int64                   `json:"finished_ns"`
	Entry      string                  `json:"entry"`
}
type armRequest struct {
	Source  string  `json:"source"`
	Fixture fixture `json:"fixture"`
}
type delivery struct {
	Receipt    string         `json:"receipt_id"`
	History    string         `json:"history_id"`
	ReceivedMS int64          `json:"received_ms"`
	VerifiedNS int64          `json:"verified_ns"`
	Count      int            `json:"count"`
	Actual     []expectedPart `json:"verified_parts"`
}
type historySnapshot struct {
	Items []api.HistoryItem `json:"items"`
}
type cliRequest struct {
	Args         []string `json:"args"`
	AllowFailure bool     `json:"allow_failure,omitempty"`
}
type cliResult struct {
	Code int    `json:"exit_code"`
	Out  []byte `json:"stdout"`
	Err  []byte `json:"stderr"`
}
type logsResult struct {
	Bytes          int    `json:"bytes"`
	SHA            string `json:"sha256"`
	ContainsSecret bool   `json:"contains_secret"`
	ForcedStop     bool   `json:"forced_stop"`
	Removed        bool   `json:"removed"`
}

type waitRequest struct {
	TimeoutSeconds int `json:"timeout_seconds"`
}
