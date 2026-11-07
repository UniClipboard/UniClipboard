package engine

import (
	"context"
	"errors"
	"fmt"

	"unic/internal/rendezvous"
)

// Code is a stable, machine-readable error class. Programs branch on Code;
// Message is for people only.
type Code string

const (
	CodeInvalidArgument   Code = "invalid_argument"
	CodeInvalidState      Code = "invalid_state"
	CodeClosed            Code = "closed"
	CodeInUse             Code = "in_use"
	CodeConflict          Code = "conflict"
	CodeNotFound          Code = "not_found"
	CodeResourceExpired   Code = "resource_expired"
	CodeUnsupported       Code = "unsupported"
	CodePermissionDenied  Code = "permission_denied"
	CodeSyncDisabled      Code = "sync_disabled"
	CodeSuspended         Code = "suspended"
	CodeNoTargets         Code = "no_targets"
	CodeLimitExceeded     Code = "limit_exceeded"
	CodeResourceExhausted Code = "resource_exhausted"
	CodeUnavailable       Code = "unavailable"
	CodeTimeout           Code = "timeout"
	CodeCancelled         Code = "cancelled"
	CodeIdentityMismatch  Code = "identity_mismatch"
	CodeProtocolMismatch  Code = "protocol_mismatch"
	CodeSourceChanged     Code = "source_changed"
	CodeIncomplete        Code = "incomplete"   // pairing code: not six digits
	CodeInvalid           Code = "invalid"      // pairing code: not a unic code
	CodeExpired           Code = "expired"      // pairing code: unknown, expired or used
	CodeRateLimited       Code = "rate_limited" // pairing code: too many tries
	CodeResyncRequired    Code = "resync_required"
	CodeInternal          Code = "internal"
)

// Error is the only error type the engine returns (use errors.As). Message is
// redacted: it never carries addresses, tickets, clipboard text, paths or the
// text of lower-level errors.
type Error struct {
	Code         Code   `json:"code"`
	Message      string `json:"message"`
	Retryable    bool   `json:"retryable,omitempty"`
	RetryAfterMs int64  `json:"retry_after_ms,omitempty"`

	cause error // only ever context.Canceled / DeadlineExceeded
}

func (e *Error) Error() string { return string(e.Code) + ": " + e.Message }

// Unwrap exposes context.Canceled and context.DeadlineExceeded so that
// errors.Is works for them; no other cause is ever attached.
func (e *Error) Unwrap() error { return e.cause }

func newErr(code Code, msg string) *Error { return &Error{Code: code, Message: msg} }

func errf(code Code, format string, a ...any) *Error {
	return &Error{Code: code, Message: fmt.Sprintf(format, a...)}
}

func retryable(e *Error) *Error { e.Retryable = true; return e }

var errClosed = newErr(CodeClosed, "the engine is closed")

// ctxErr maps a context failure.
func ctxErr(err error) *Error {
	if errors.Is(err, context.DeadlineExceeded) {
		e := retryable(newErr(CodeTimeout, "the operation timed out"))
		e.cause = context.DeadlineExceeded
		return e
	}
	e := newErr(CodeCancelled, "the operation was cancelled")
	e.cause = context.Canceled
	return e
}

// codeFailure maps a rendezvous failure to the pairing code error classes.
func codeFailure(err error) (*Error, bool) {
	switch rendezvous.ReasonOf(err) {
	case rendezvous.Incomplete:
		return newErr(CodeIncomplete, "enter all 6 digits of the code"), true
	case rendezvous.Invalid:
		return newErr(CodeInvalid, "that is not a unic pairing code"), true
	case rendezvous.Expired:
		return newErr(CodeExpired, "this code has expired or was already used; ask for a new one"), true
	case rendezvous.RateLimited:
		e := retryable(newErr(CodeRateLimited, "too many tries; wait a minute and try again"))
		e.RetryAfterMs = 60_000
		return e, true
	case rendezvous.Unavailable:
		return retryable(newErr(CodeUnavailable, "the pairing code service is unavailable; pair with the full address instead")), true
	}
	return nil, false
}

// asError returns err as an *Error, replacing anything else by a redacted one.
func asError(err error) *Error {
	var e *Error
	if errors.As(err, &e) {
		return e
	}
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return ctxErr(err)
	}
	return newErr(CodeInternal, "internal error")
}
