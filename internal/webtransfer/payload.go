package webtransfer

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"regexp"
)

// Limits shared with the browser page, so both sides agree on what is acceptable.
const (
	MaxTransferBytes = 20 * 1024 * 1024
	MaxItems         = 50
	maxNameChars     = 255
	maxMimeChars     = 255
	protocolVersion  = 1
)

// Kind of one item.
type Kind string

const (
	KindText  Kind = "text"
	KindImage Kind = "image"
	KindFile  Kind = "file"
)

// Meta describes one item. Name is empty for text. Sizes are bytes.
type Meta struct {
	Kind   Kind   `json:"kind"`
	Name   string `json:"name"`
	MIME   string `json:"mime"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
}

// Item is one item with its bytes.
type Item struct {
	Meta
	Data []byte
}

// Offer is the OFFER control message.
type Offer struct {
	TransferID string `json:"transferId"`
	Items      []Meta `json:"items"`
	Total      int64  `json:"total"`
}

var (
	// ErrTooLarge: the payload is over MaxTransferBytes or MaxItems.
	ErrTooLarge = errors.New("webtransfer: payload exceeds the transfer limit")
	// ErrBusy: the sender already serves another receiver.
	ErrBusy = errors.New("webtransfer: the sender is busy")
	// ErrRejected: the peer refused the content.
	ErrRejected = errors.New("webtransfer: the peer rejected the content")
	// ErrCancelled: the peer cancelled the transfer.
	ErrCancelled = errors.New("webtransfer: the peer cancelled")
	// ErrIntegrity: a SHA-256 did not match.
	ErrIntegrity = errors.New("webtransfer: content failed verification")
	// ErrProtocol: the peer broke the protocol.
	ErrProtocol = errors.New("webtransfer: protocol error")
)

var sha256Pattern = regexp.MustCompile(`^[0-9a-f]{64}$`)

// Prepare builds the offer for items, filling sizes and hashes. Items need only
// Kind, Name, MIME and Data.
func Prepare(items []Item) (Offer, []Item, error) {
	if len(items) == 0 || len(items) > MaxItems {
		return Offer{}, nil, ErrTooLarge
	}
	out := make([]Item, len(items))
	var total int64
	for i, it := range items {
		if it.Kind != KindText && it.Kind != KindImage && it.Kind != KindFile {
			return Offer{}, nil, ErrProtocol
		}
		sum := sha256.Sum256(it.Data)
		out[i] = Item{Meta: Meta{Kind: it.Kind, Name: truncate(it.Name, maxNameChars), MIME: truncate(it.MIME, maxMimeChars), Size: int64(len(it.Data)), SHA256: hex.EncodeToString(sum[:])}, Data: it.Data}
		total += out[i].Size
		if total > MaxTransferBytes {
			return Offer{}, nil, ErrTooLarge
		}
	}
	offer := Offer{TransferID: RandomID(16), Total: total}
	for _, it := range out {
		offer.Items = append(offer.Items, it.Meta)
	}
	return offer, out, nil
}

func truncate(s string, n int) string {
	r := []rune(s)
	if len(r) > n {
		return string(r[:n])
	}
	return s
}

// RandomID returns n random bytes as unpadded base64url, the browser's ID form.
func RandomID(n int) string {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		panic("webtransfer: no randomness: " + err.Error())
	}
	return base64.RawURLEncoding.EncodeToString(b)
}

// validOffer checks an untrusted OFFER the way the browser does. The size limit
// is checked separately so the caller can answer with a specific REJECT.
func validOffer(o Offer) bool {
	if o.TransferID == "" || len(o.TransferID) > 64 || o.Total < 0 || len(o.Items) > MaxItems {
		return false
	}
	var sum int64
	for _, m := range o.Items {
		if m.Kind != KindText && m.Kind != KindImage && m.Kind != KindFile {
			return false
		}
		if len([]rune(m.Name)) > maxNameChars || len([]rune(m.MIME)) > maxMimeChars || m.Size < 0 || !sha256Pattern.MatchString(m.SHA256) {
			return false
		}
		sum += m.Size
		if sum < 0 || sum > 1<<53 {
			return false
		}
	}
	return sum == o.Total
}

func jsonDecode(r io.Reader, v any) error {
	d := json.NewDecoder(r)
	return d.Decode(v)
}
