package mobile

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/v1migration"
)

var errTargetBusy = errors.New("migration target is in use")

type mobileError struct {
	Code         string `json:"code"`
	Message      string `json:"message"`
	Retryable    bool   `json:"retryable,omitempty"`
	RetryAfterMs int64  `json:"retry_after_ms,omitempty"`
}

func toMobileError(err error) *mobileError {
	if err == nil {
		return nil
	}
	var ee *engine.Error
	if errors.As(err, &ee) {
		return &mobileError{
			Code:         string(ee.Code),
			Message:      ee.Message,
			Retryable:    ee.Retryable,
			RetryAfterMs: ee.RetryAfterMs,
		}
	}
	switch {
	case errors.Is(err, errTargetBusy):
		return &mobileError{Code: string(engine.CodeInUse), Message: "the data directory is used by another task", Retryable: true}
	case errors.Is(err, v1migration.ErrUnlock):
		return &mobileError{Code: string(engine.CodePermissionDenied), Message: "could not unlock v1 credentials; enter the old passphrase"}
	case errors.Is(err, v1migration.ErrFormat):
		return &mobileError{Code: string(engine.CodeInvalidArgument), Message: "unsupported or invalid v1 data directory"}
	case errors.Is(err, v1migration.ErrContent):
		return &mobileError{Code: string(engine.CodeUnavailable), Message: "v1 data could not be read", Retryable: true}
	case errors.Is(err, context.Canceled):
		return &mobileError{Code: string(engine.CodeCancelled), Message: "the operation was cancelled"}
	case errors.Is(err, context.DeadlineExceeded):
		return &mobileError{Code: string(engine.CodeTimeout), Message: "the operation timed out", Retryable: true}
	}
	return &mobileError{Code: string(engine.CodeInternal), Message: "internal error"}
}

func wrapErr(err error) error {
	me := toMobileError(err)
	if me == nil {
		return nil
	}
	b, _ := json.Marshal(me)
	return errors.New(string(b))
}
