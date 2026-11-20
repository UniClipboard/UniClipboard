package quickpanel

import "errors"

// FailureCode carries only stable public categories across private role IPC.
func FailureCode(err error) string {
	switch {
	case err == nil:
		return ""
	case errors.Is(err, ErrUnsupported):
		return "unsupported"
	case errors.Is(err, ErrPermission):
		return "permission"
	case errors.Is(err, ErrProtected):
		return "protected"
	case errors.Is(err, ErrFocus):
		return "focus"
	case errors.Is(err, ErrPlain):
		return "plain"
	case errors.Is(err, ErrClipboard):
		return "clipboard"
	case errors.Is(err, ErrInput):
		return "input"
	default:
		return "target"
	}
}
func FailureFromCode(code string) error {
	switch code {
	case "":
		return nil
	case "unsupported":
		return ErrUnsupported
	case "permission":
		return ErrPermission
	case "protected":
		return ErrProtected
	case "focus":
		return ErrFocus
	case "plain":
		return ErrPlain
	case "clipboard":
		return ErrClipboard
	case "input":
		return ErrInput
	default:
		return ErrTarget
	}
}
