package relaycrypto

import (
	"encoding/binary"
	"encoding/json"
	"io"

	"github.com/uniclipboard/unic/internal/wire"
)

// EncodePlaintextHeader encodes u32_be(len(clip_json)) || clip_json.
func EncodePlaintextHeader(clip wire.Clip) ([]byte, error) {
	if err := clip.Validate(); err != nil {
		return nil, ErrInvalidInput
	}
	raw, err := json.Marshal(clip)
	if err != nil {
		return nil, err
	}
	if len(raw) == 0 || len(raw) > wire.MaxControl {
		return nil, ErrInvalidInput
	}
	out := make([]byte, 4+len(raw))
	binary.BigEndian.PutUint32(out[0:4], uint32(len(raw)))
	copy(out[4:], raw)
	return out, nil
}

// ReadPlaintextHeader reads u32_be(len(clip_json)) || clip_json from r.
func ReadPlaintextHeader(r io.Reader) (wire.Clip, error) {
	var hdr [4]byte
	if _, err := io.ReadFull(r, hdr[:]); err != nil {
		return wire.Clip{}, err
	}
	n := binary.BigEndian.Uint32(hdr[:])
	if n == 0 || int(n) > wire.MaxControl {
		return wire.Clip{}, ErrBadEnvelope
	}
	raw := make([]byte, n)
	if _, err := io.ReadFull(r, raw); err != nil {
		return wire.Clip{}, err
	}
	var clip wire.Clip
	if err := json.Unmarshal(raw, &clip); err != nil {
		return wire.Clip{}, ErrBadEnvelope
	}
	if clip.T != "clip" || clip.Validate() != nil {
		return wire.Clip{}, ErrBadEnvelope
	}
	return clip, nil
}

// NewBitmap returns a zeroed little-endian LSB-first bitmap for totalChunks.
func NewBitmap(totalChunks int) ([]byte, error) {
	if totalChunks <= 0 || totalChunks > MaxChunks {
		return nil, ErrInvalidInput
	}
	return make([]byte, (totalChunks+7)/8), nil
}

// SetBit sets chunk index in bm.
func SetBit(bm []byte, index int) {
	if index < 0 || index/8 >= len(bm) {
		return
	}
	bm[index/8] |= 1 << uint(index%8)
}

// ClearBit clears chunk index in bm.
func ClearBit(bm []byte, index int) {
	if index < 0 || index/8 >= len(bm) {
		return
	}
	bm[index/8] &^= 1 << uint(index%8)
}

// HasBit reports whether chunk index is set in bm.
func HasBit(bm []byte, index int) bool {
	if index < 0 || index/8 >= len(bm) {
		return false
	}
	return (bm[index/8] & (1 << uint(index%8))) != 0
}

// CountBits returns the number of set bits in [0, totalChunks).
func CountBits(bm []byte, totalChunks int) int {
	n := 0
	for i := 0; i < totalChunks; i++ {
		if HasBit(bm, i) {
			n++
		}
	}
	return n
}

// EncodeBitmap encodes bm as unpadded base64url.
func EncodeBitmap(bm []byte) string { return b64.EncodeToString(bm) }

// DecodeBitmap decodes a base64url bitmap and rejects invalid length or non-zero trailing bits.
func DecodeBitmap(encoded string, totalChunks int) ([]byte, error) {
	if totalChunks <= 0 || totalChunks > MaxChunks {
		return nil, ErrInvalidInput
	}
	raw, err := b64.DecodeString(encoded)
	if err != nil {
		return nil, ErrInvalidInput
	}
	wantLen := (totalChunks + 7) / 8
	if len(raw) != wantLen {
		return nil, ErrInvalidInput
	}
	if rem := totalChunks % 8; rem != 0 {
		mask := byte(0xff << uint(rem))
		if raw[wantLen-1]&mask != 0 {
			return nil, ErrInvalidInput
		}
	}
	return raw, nil
}
