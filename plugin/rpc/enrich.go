package rpc

import (
	"encoding/json"
	"regexp"
	"unicode/utf8"
)

const (
	DefaultEnrichTimeoutMs          = 15000
	MaxEnrichTimeoutMs              = 120000
	MaxEnrichSearchDocs             = 16
	MaxEnrichSearchDocBytes         = 64 << 10
	MaxEnrichSearchTotalBytes       = 256 << 10
	MaxEnrichAnnotations            = 8
	MaxEnrichAnnotationPayloadBytes = 256 << 10
	MaxEnrichAssetsPerAnnotation    = 8
	MaxEnrichAssetBytes             = 256 << 10
	MaxEnrichTotalAssetBytes        = 512 << 10
)

var (
	annotationTypeRe = regexp.MustCompile(`^[a-z][a-z0-9_]*\.v[1-9][0-9]*$`)
	enrichDocKindRe  = regexp.MustCompile(`^[a-z][a-z0-9_]{0,31}$`)
	enrichAssetKeyRe = regexp.MustCompile(`^[a-z][a-z0-9_-]{0,31}$`)
)

// ValidAnnotationType reports whether t is a canonical versioned annotation type.
func ValidAnnotationType(t string) bool {
	return len(t) <= 64 && annotationTypeRe.MatchString(t)
}

// ValidEnrichDocKind reports whether k is a valid derived search document kind.
func ValidEnrichDocKind(k string) bool {
	return k != "" && k != "base" && enrichDocKindRe.MatchString(k)
}

// ValidEnrichAssetKey reports whether k is a valid annotation asset key.
func ValidEnrichAssetKey(k string) bool {
	return enrichAssetKeyRe.MatchString(k)
}

// ValidEnrichAssetMIME reports whether m is an allowed binary asset MIME type.
func ValidEnrichAssetMIME(m string) bool {
	switch m {
	case "image/png", "image/webp", "image/jpeg":
		return true
	}
	return false
}

// Enrich is the wire request for plugin.enrich.
type Enrich struct {
	Trigger    string       `json:"trigger"`
	Generation U64          `json:"generation"`
	Item       Item         `json:"item"`
	Parts      []ActionPart `json:"parts,omitempty"`
}

func (e Enrich) Validate() error {
	switch e.Trigger {
	case "on_commit", "on_demand", "manual":
	default:
		return protoErr("bad enrich trigger")
	}
	if len(e.Parts) > 4096 {
		return protoErr("too many enrich parts")
	}
	for _, p := range e.Parts {
		if p.Size > U64(1<<63-1) || len(p.MIME) > 256 || !utf8.ValidString(p.MIME) || len(p.Name) > 4096 || !utf8.ValidString(p.Name) {
			return protoErr("bad enrich part")
		}
	}
	return e.Item.validateText(PurposeEnrichText)
}

type EnrichSearchDoc struct {
	Kind string `json:"kind"`
	Text string `json:"text"`
}

type EnrichAssetDesc struct {
	Key    string     `json:"key"`
	MIME   string     `json:"mime"`
	Stream StreamDesc `json:"stream"`
}

type EnrichAnnotation struct {
	Type    string            `json:"type"`
	Payload json.RawMessage   `json:"payload"`
	Assets  []EnrichAssetDesc `json:"assets,omitempty"`
}

// EnrichResult is the wire response for plugin.enrich.
type EnrichResult struct {
	Generation  U64                `json:"generation"`
	Status      string             `json:"status"`
	SearchDocs  []EnrichSearchDoc  `json:"search_docs,omitempty"`
	AddTags     []string           `json:"add_tags,omitempty"`
	Annotations []EnrichAnnotation `json:"annotations,omitempty"`
}

func (r EnrichResult) Validate() error {
	switch r.Status {
	case "ok":
	case "skipped":
		if len(r.SearchDocs) != 0 || len(r.AddTags) != 0 || len(r.Annotations) != 0 {
			return NewError(CodeInvalidArgument)
		}
		return nil
	default:
		return NewError(CodeInvalidArgument)
	}
	if len(r.SearchDocs) > MaxEnrichSearchDocs {
		return NewError(CodeLimitExceeded)
	}
	totalSearch := 0
	for _, d := range r.SearchDocs {
		if !ValidEnrichDocKind(d.Kind) || d.Text == "" || !utf8.ValidString(d.Text) {
			return NewError(CodeInvalidArgument)
		}
		if len(d.Text) > MaxEnrichSearchDocBytes {
			return NewError(CodeLimitExceeded)
		}
		totalSearch += len(d.Text)
		if totalSearch > MaxEnrichSearchTotalBytes {
			return NewError(CodeLimitExceeded)
		}
	}
	if len(r.AddTags) > MaxTags {
		return NewError(CodeLimitExceeded)
	}
	seenTags := map[string]bool{}
	for _, t := range r.AddTags {
		if t == "" || !validUTF8(t, MaxTagBytes) || seenTags[t] {
			return NewError(CodeInvalidArgument)
		}
		seenTags[t] = true
	}
	if len(r.Annotations) > MaxEnrichAnnotations {
		return NewError(CodeLimitExceeded)
	}
	seenTypes := map[string]bool{}
	var totalAssets uint64
	for _, a := range r.Annotations {
		if !ValidAnnotationType(a.Type) || seenTypes[a.Type] {
			return NewError(CodeInvalidArgument)
		}
		seenTypes[a.Type] = true
		if len(a.Payload) == 0 || len(a.Payload) > MaxEnrichAnnotationPayloadBytes || !json.Valid(a.Payload) {
			return NewError(CodeInvalidArgument)
		}
		if len(a.Assets) > MaxEnrichAssetsPerAnnotation {
			return NewError(CodeLimitExceeded)
		}
		seenKeys := map[string]bool{}
		for _, asset := range a.Assets {
			if !ValidEnrichAssetKey(asset.Key) || seenKeys[asset.Key] || !ValidEnrichAssetMIME(asset.MIME) {
				return NewError(CodeInvalidArgument)
			}
			seenKeys[asset.Key] = true
			if err := asset.Stream.validate(PurposeEnrichAsset); err != nil {
				return NewError(CodeInvalidArgument)
			}
			sz := uint64(asset.Stream.Size)
			if sz == 0 || sz > MaxEnrichAssetBytes || totalAssets+sz > MaxEnrichTotalAssetBytes {
				return NewError(CodeLimitExceeded)
			}
			totalAssets += sz
		}
	}
	return nil
}
