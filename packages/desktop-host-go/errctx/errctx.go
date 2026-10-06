// Package errctx mirrors anyhow's `.context(msg)`: the error displays only the
// outermost message while the cause stays reachable through errors.Unwrap.
// The Rust CLI prints errors with `{err}`, which shows just that message.
package errctx

type contextError struct {
	msg   string
	cause error
}

func (e *contextError) Error() string { return e.msg }
func (e *contextError) Unwrap() error { return e.cause }

// Wrap attaches msg as the displayed context of cause.
func Wrap(msg string, cause error) error { return &contextError{msg: msg, cause: cause} }
