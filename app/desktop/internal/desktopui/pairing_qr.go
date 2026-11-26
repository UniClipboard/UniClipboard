package desktopui

import (
	"image"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/skip2/go-qrcode"
)

const (
	pairingQRMinPixels   = 512
	maxPairingQRBytes    = 2953 // QR version 40 byte capacity at Low recovery level.
	pairingCodeQRSize    = 126
	pairingCodeQRPadding = 3
	pairingTicketQRSize  = 152
	pairingTicketQRPad   = 3
)

// pairingQRCache caches the current invitation QR bitmap for the exact secret
// payload currently displayed, avoiding per-frame re-encoding while ensuring
// close/cancel/expiry drops both the bitmap and the payload string.
type pairingQRCache struct {
	payload string
	bitmap  *ui.Bitmap
}

// encodeQRMatrixImage renders a local, integer-scaled QR code with the standard
// four-module white quiet zone. It never performs network I/O or logs payload.
func encodeQRMatrixImage(payload string, level qrcode.RecoveryLevel, minPixels int) *image.NRGBA {
	if payload == "" || minPixels <= 0 {
		return nil
	}
	code, err := qrcode.New(payload, level)
	if err != nil {
		return nil
	}
	matrix := code.Bitmap()
	n := len(matrix)
	if n == 0 {
		return nil
	}
	scale := max(1, (minPixels+n-1)/n)
	side := n * scale
	img := image.NewNRGBA(image.Rect(0, 0, side, side))
	for i := 0; i < len(img.Pix); i += 4 {
		img.Pix[i] = 255
		img.Pix[i+1] = 255
		img.Pix[i+2] = 255
		img.Pix[i+3] = 255
	}
	for row := 0; row < n; row++ {
		line := matrix[row]
		for col := 0; col < len(line); col++ {
			if !line[col] {
				continue
			}
			y0 := row * scale
			x0 := col * scale
			for dy := 0; dy < scale; dy++ {
				off := (y0+dy)*img.Stride + x0*4
				for dx := 0; dx < scale; dx++ {
					p := off + dx*4
					img.Pix[p] = 0
					img.Pix[p+1] = 0
					img.Pix[p+2] = 0
					img.Pix[p+3] = 255
				}
			}
		}
	}
	return img
}

// encodePairingQRImage encodes a 6-digit pairing code or a unic-pair: ticket
// into a local, crisp integer-scaled QR image suitable for mobile scanning.
// Non-pairing strings and oversized payloads return nil.
func encodePairingQRImage(raw string) *image.NRGBA {
	s := strings.TrimSpace(raw)
	switch {
	case pairCodePattern.MatchString(s):
		return encodeQRMatrixImage(s, qrcode.Medium, pairingQRMinPixels)
	case pairTicketPattern.MatchString(s) && len(s) <= maxPairingQRBytes:
		return encodeQRMatrixImage(s, qrcode.Low, pairingQRMinPixels)
	default:
		return nil
	}
}

func (d *viewState) pairingQRBitmap(raw string) *ui.Bitmap {
	s := strings.TrimSpace(raw)
	if s == "" {
		d.clearPairingQR()
		return nil
	}
	if d.pairingQR.payload == s && d.pairingQR.bitmap != nil {
		return d.pairingQR.bitmap
	}
	img := encodePairingQRImage(s)
	if img == nil {
		d.clearPairingQR()
		return nil
	}
	bm := ui.NewBitmap(img)
	d.pairingQR = pairingQRCache{payload: s, bitmap: bm}
	return bm
}

func (d *viewState) clearPairingQR() {
	d.pairingQR = pairingQRCache{}
}
