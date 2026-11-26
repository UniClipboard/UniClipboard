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
	Trees []treeRoot     `json:"trees,omitempty"`
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
	LastOperation          *engine.Operation       `json:"last_observed_operation,omitempty"`
	InitialCLIResultKnown  bool                    `json:"initial_cli_result_known"`
	InitialCLIExit         int                     `json:"initial_cli_exit"`
	InitialSendOutcome     string                  `json:"initial_send_outcome"`
	SettledAfterTimeout    bool                    `json:"settled_after_cli_timeout"`
	PreparationAdmissionMS float64                 `json:"preparation_and_admission_ms"`
	SendOperationMS        float64                 `json:"send_operation_ms"`
	Resources              *directoryMeasure       `json:"resources,omitempty"`
	Result                 engine.SubmissionResult `json:"result"`
	StartedNS              int64                   `json:"started_ns"`
	FinishedNS             int64                   `json:"finished_ns"`
	Entry                  string                  `json:"entry"`
}
type armRequest struct {
	Source  string  `json:"source"`
	Fixture fixture `json:"fixture"`
}
type delivery struct {
	Resources  *directoryMeasure `json:"resources,omitempty"`
	Trees      []treeRoot        `json:"verified_trees,omitempty"`
	Receipt    string            `json:"receipt_id"`
	History    string            `json:"history_id"`
	ReceivedMS int64             `json:"received_ms"`
	VerifiedNS int64             `json:"verified_ns"`
	Count      int               `json:"count"`
	Actual     []expectedPart    `json:"verified_parts"`
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
	InjectedCrashes int    `json:"injected_crashes"`
	Bytes           int    `json:"bytes"`
	SHA             string `json:"sha256"`
	ContainsSecret  bool   `json:"contains_secret"`
	ForcedStop      bool   `json:"forced_stop"`
	Removed         bool   `json:"removed"`
}

type waitRequest struct {
	TimeoutSeconds int `json:"timeout_seconds"`
}

type observeAutoRequest struct {
	Fixture string `json:"fixture"`
}

type observeAutoResult struct {
	Disposition engine.Disposition `json:"disposition"`
	OperationID engine.OperationID `json:"operation_id,omitempty"`
	Outcome     engine.Outcome     `json:"outcome,omitempty"`
	ElapsedMS   float64            `json:"elapsed_ms"`
}

type sendProgressRequest struct {
	Fixture        string `json:"fixture"`
	Key            string `json:"key"`
	TimeoutSeconds int    `json:"timeout_seconds"`
}

type progressSample struct {
	State        engine.TargetState  `json:"state"`
	ReceiptStage engine.ReceiptStage `json:"receipt_stage,omitempty"`
	BytesSent    int64               `json:"bytes_sent"`
	TotalBytes   int64               `json:"total_bytes"`
	PartIndex    int                 `json:"part_index,omitempty"`
	PartCount    int                 `json:"part_count,omitempty"`
}

type sendProgressResult struct {
	HistoryID           engine.HistoryID    `json:"history_id,omitempty"`
	OperationID         engine.OperationID  `json:"operation_id,omitempty"`
	OperationHistoryID  engine.HistoryID    `json:"operation_history_id,omitempty"`
	FoundByHistoryQuery bool                `json:"found_by_history_query"`
	FoundByActiveQuery  bool                `json:"found_by_active_query"`
	OperationEvents     int                 `json:"operation_events"`
	Monotonic           bool                `json:"monotonic"`
	SawIntermediate     bool                `json:"saw_intermediate"`
	SawAwaitingReceipt  bool                `json:"saw_awaiting_receipt"`
	Samples             []progressSample    `json:"samples,omitempty"`
	Outcome             engine.Outcome      `json:"outcome,omitempty"`
	FinalState          engine.TargetState  `json:"final_state,omitempty"`
	FinalReceiptStage   engine.ReceiptStage `json:"final_receipt_stage,omitempty"`
	FinalBytesSent      int64               `json:"final_bytes_sent"`
	FinalTotalBytes     int64               `json:"final_total_bytes"`
	FinalErrorCode      engine.Code         `json:"final_error_code,omitempty"`
	ElapsedMS           float64             `json:"elapsed_ms"`
}

type channelPolicyRequest struct {
	Text   bool `json:"text"`
	Images bool `json:"images"`
	Files  bool `json:"files"`
}
