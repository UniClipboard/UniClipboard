// Package hostapi holds the host command error contract shared by the Go service and the generated frontend files.
//
// Wails generates TypeScript for method parameters, results and registered events, but a bound method only returns
// `error`, so the framework cannot say which codes a command may reject with. This package is therefore the single
// source for every error code the host puts on the wire, with its severity (user-facing codes are normal product flow
// and are never reported to Sentry; any other rejection is a system error). `cmd/hosterrors` exports the catalog to
// the frontend; nothing else lists codes.
package hostapi

import (
	"encoding/json"
	"errors"
)

// CommandCode is the `code` of a CommandError.
type CommandCode string

const (
	CodeNotFound                        CommandCode = "NotFound"
	CodeInternalError                   CommandCode = "InternalError"
	CodeTimeout                         CommandCode = "Timeout"
	CodeCancelled                       CommandCode = "Cancelled"
	CodeValidationError                 CommandCode = "ValidationError"
	CodeConflict                        CommandCode = "Conflict"
	CodeAccessibilityPermissionRequired CommandCode = "AccessibilityPermissionRequired"
)

// UnlockCode is the `code` of an UnlockError (the content unlock refusal reasons).
type UnlockCode string

const (
	UnlockWrongPassphrase         UnlockCode = "WRONG_PASSPHRASE"
	UnlockCorruptedKeyMaterial    UnlockCode = "CORRUPTED_KEY_MATERIAL"
	UnlockSetupNotCompleted       UnlockCode = "SETUP_NOT_COMPLETED"
	UnlockSpaceNotInitialized     UnlockCode = "SPACE_NOT_INITIALIZED"
	UnlockProfileRecoveryRequired UnlockCode = "PROFILE_RECOVERY_REQUIRED"
	UnlockProfileRecoveryPartial  UnlockCode = "PROFILE_RECOVERY_PARTIAL"
	UnlockProfileRecoveryUnsup    UnlockCode = "PROFILE_RECOVERY_UNSUPPORTED"
	UnlockProfileRecoveryPersist  UnlockCode = "PROFILE_RECOVERY_PERSISTENCE_FAILED"
	UnlockInternal                UnlockCode = "INTERNAL"
)

// ConfigKind is the `kind` of a ConfigError, the tagged union of the config package commands.
type ConfigKind string

const (
	ConfigCancelled ConfigKind = "cancelled"
	ConfigDaemon    ConfigKind = "daemon"
	ConfigInternal  ConfigKind = "internal"
)

// Severity classes. A class is decided here and nowhere else.
type Severity string

const (
	// UserFacing rejections are expected product flow (wrong passphrase, cancelled dialog, bad input).
	UserFacing Severity = "user"
	// System rejections are unexpected and are reported.
	System Severity = "system"
)

// CodeInfo is one catalog row: a wire token and its severity.
type CodeInfo struct {
	Token    string
	Severity Severity
}

// Catalog lists every token the host can reject with, grouped by the wire family that carries it. A token that is
// not here cannot be classified, and the frontend treats it as a system error (fail safe toward visibility).
var Catalog = struct {
	Command []CodeInfo
	Unlock  []CodeInfo
	Config  []CodeInfo
}{
	Command: []CodeInfo{
		{string(CodeNotFound), UserFacing},
		{string(CodeInternalError), System},
		{string(CodeTimeout), System},
		{string(CodeCancelled), UserFacing},
		{string(CodeValidationError), UserFacing},
		{string(CodeConflict), UserFacing},
		{string(CodeAccessibilityPermissionRequired), UserFacing},
	},
	Unlock: []CodeInfo{
		{string(UnlockWrongPassphrase), UserFacing},
		{string(UnlockCorruptedKeyMaterial), System},
		{string(UnlockSetupNotCompleted), UserFacing},
		{string(UnlockSpaceNotInitialized), UserFacing},
		{string(UnlockProfileRecoveryRequired), UserFacing},
		{string(UnlockProfileRecoveryPartial), System},
		{string(UnlockProfileRecoveryUnsup), System},
		{string(UnlockProfileRecoveryPersist), System},
		{string(UnlockInternal), System},
	},
	// Config rejections are told apart by `kind`: a cancelled dialog is the only user-facing one.
	Config: []CodeInfo{
		{string(ConfigCancelled), UserFacing},
		{string(ConfigDaemon), System},
		{string(ConfigInternal), System},
	},
}

// CommandError is the rejection of most commands: `{ code, message }`.
type CommandError struct {
	Code    CommandCode `json:"code"`
	Message string      `json:"message"`
}

func (e CommandError) Error() string { return string(e.Code) + ": " + e.Message }

// UnlockError is the rejection of `unlock_content`: only the stable code crosses the bridge, because the daemon's
// text may contain private data.
type UnlockError struct {
	Code UnlockCode `json:"code"`
}

func (e UnlockError) Error() string { return string(e.Code) }

// ConfigError is the rejection of the config package commands: `{kind:"cancelled"}`,
// `{kind:"daemon", status, code, message}` or `{kind:"internal", message}`.
type ConfigError struct {
	Kind    ConfigKind `json:"kind"`
	Status  *int       `json:"status,omitempty"`
	Code    *string    `json:"code,omitempty"`
	Message string     `json:"message,omitempty"`
}

func (e ConfigError) Error() string { return string(e.Kind) + ": " + e.Message }

// TextError is the rejection of the update, quick panel and window commands: a plain string on the wire (the former
// `Result<_, String>` commands). It has no code, so the frontend always treats it as a system error.
type TextError string

func (e TextError) Error() string { return string(e) }

// New builds a CommandError. Passing a catalog constant (not a string literal) is enforced by cmd/hosterrors.
func New(code CommandCode, message string) error { return CommandError{Code: code, Message: message} }

// Internal wraps an arbitrary failure as an InternalError.
func Internal(err error) error {
	if err == nil {
		return nil
	}
	return CommandError{Code: CodeInternalError, Message: err.Error()}
}

// Marshal is the Wails service-level MarshalError: it keeps the typed rejections as their wire shape and wraps
// anything else as an InternalError, so the frontend always receives an object with a stable `code` (Wails' default
// marshals a plain error to `{}`). A code outside the catalog is also downgraded to InternalError, so an unlisted
// token can never reach the frontend as if it had been classified.
func Marshal(err error) []byte {
	var out any
	var command CommandError
	var unlock UnlockError
	var config ConfigError
	var text TextError
	switch {
	case errors.As(err, &command):
		out = command
		if !known(Catalog.Command, string(command.Code)) {
			out = CommandError{Code: CodeInternalError, Message: command.Message}
		}
	case errors.As(err, &unlock):
		out = unlock
		if !known(Catalog.Unlock, string(unlock.Code)) {
			out = UnlockError{Code: UnlockInternal}
		}
	case errors.As(err, &config):
		out = config
		if !known(Catalog.Config, string(config.Kind)) {
			out = ConfigError{Kind: ConfigInternal, Message: config.Message}
		}
	case errors.As(err, &text):
		out = string(text)
	default:
		out = CommandError{Code: CodeInternalError, Message: err.Error()}
	}
	raw, marshalErr := json.Marshal(out)
	if marshalErr != nil {
		raw, _ = json.Marshal(CommandError{Code: CodeInternalError, Message: "failed to encode the error"})
	}
	return raw
}

func known(family []CodeInfo, token string) bool {
	for _, info := range family {
		if info.Token == token {
			return true
		}
	}
	return false
}

// UnlockFromDaemon maps the daemon's error token to an UnlockCode; a token outside the catalog becomes INTERNAL.
func UnlockFromDaemon(token string) UnlockCode {
	if known(Catalog.Unlock, token) {
		return UnlockCode(token)
	}
	return UnlockInternal
}
