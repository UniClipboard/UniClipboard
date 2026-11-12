package rpc

import (
	"unicode/utf8"

	"github.com/uniclipboard/unic/sourceapp"
)

// Single-target send for peer_sessions backends (sync-backend-contract §4).
const (
	MethodSend          = "backend.send"             // Engine→backend
	MethodAuthorizeSend = "sync_host.authorize_send" // backend→Engine
	MethodOpenSendPart  = "sync_host.open_send_part" // backend→Engine
	MaxClipParts        = 4096
	MaxClipName         = 1024
	SendSent            = "sent"
)

// ClipPart describes one wire part, in wire order; Index runs from 0.
type ClipPart struct {
	Index int    `json:"index"`
	Kind  string `json:"kind"` // representation or file
	MIME  string `json:"mime"`
	Name  string `json:"name"`
	Size  U64    `json:"size"`
}

// Clip is the wire clip header the backend writes after being authorized.
type Clip struct {
	SourceApplication *sourceapp.Application `json:"source_application,omitempty"`
	WireID            string                 `json:"wire_id"`
	Origin            string                 `json:"origin"`
	AtMs              U64                    `json:"at_ms"`
	Hash              string                 `json:"hash"`
	Parts             []ClipPart             `json:"parts"`
}

func (c Clip) Validate() error {
	if c.SourceApplication.Validate() != nil {
		return NewError(CodeInvalidArgument)
	}
	if !validID(c.WireID) || !validID(c.Origin) || !hex64Re.MatchString(c.Hash) || len(c.Parts) == 0 || len(c.Parts) > MaxClipParts {
		return NewError(CodeInvalidArgument)
	}
	for i, p := range c.Parts {
		if p.Index != i || (p.Kind != "representation" && p.Kind != "file") || len(p.MIME) > 128 || len(p.Name) > MaxClipName ||
			!utf8.ValidString(p.MIME) || !utf8.ValidString(p.Name) || (p.Kind == "representation" && p.MIME == "") {
			return NewError(CodeInvalidArgument)
		}
	}
	return nil
}

// Send is one dispatch to one connected peer. The tokens are secrets scoped to
// this call.
type Send struct {
	DispatchID    string `json:"dispatch_id"`
	OperationID   string `json:"operation_id"`
	PeerID        string `json:"peer_id"`
	ConnectionID  string `json:"connection_id"`
	SessionToken  string `json:"session_token"`
	ContentHandle string `json:"content_handle"`
	Clip          Clip   `json:"clip"`
	BackendID     string `json:"backend_id"`
	Mode          string `json:"mode"` // automatic or explicit
}

func (s Send) Validate() error {
	if !validID(s.DispatchID) || !validID(s.OperationID) || !validID(s.PeerID) || !validID(s.ConnectionID) || !validID(s.BackendID) ||
		!hex64Re.MatchString(s.SessionToken) || !hex64Re.MatchString(s.ContentHandle) || (s.Mode != "automatic" && s.Mode != "explicit") {
		return NewError(CodeInvalidArgument)
	}
	return s.Clip.Validate()
}

// Authorize is the last check before the first wire byte; it repeats the
// identifiers of the backend.send it belongs to.
type Authorize struct {
	DispatchID    string `json:"dispatch_id"`
	ContentHandle string `json:"content_handle"`
	ConnectionID  string `json:"connection_id"`
	SessionToken  string `json:"session_token"`
}

func (a Authorize) Validate() error {
	if !validID(a.DispatchID) || !validID(a.ConnectionID) || !hex64Re.MatchString(a.ContentHandle) || !hex64Re.MatchString(a.SessionToken) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type SendGrant struct {
	GrantID       string `json:"grant_id"`
	StateRevision U64    `json:"state_revision"`
}

func (g SendGrant) Validate() error {
	if !hex64Re.MatchString(g.GrantID) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type OpenSendPart struct {
	GrantID string `json:"grant_id"`
	Index   int    `json:"index"`
}

func (o OpenSendPart) Validate() error {
	if !hex64Re.MatchString(o.GrantID) || o.Index < 0 || o.Index >= MaxClipParts {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type SendResult struct {
	State string `json:"state"`
}

func (r SendResult) Validate() error {
	if r.State != SendSent {
		return NewError(CodeInvalidArgument)
	}
	return nil
}
