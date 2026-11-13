package board

import (
	"bytes"
	"image"
	"image/png"
	"sync"
	"time"

	"golang.org/x/image/draw"

	"github.com/uniclipboard/unic/sourceapp"
)

type sourceState struct {
	token uint64
	app   *sourceapp.Application
}

type iconEntry struct {
	data []byte
	at   time.Time
}

var appIcons = struct {
	sync.Mutex
	entries map[string]iconEntry
}{entries: map[string]iconEntry{}}

// Cache extraction, including missing icons, for five minutes. Historical icons
// are immutable snapshots and persist under the Engine's history budget.
func cachedIcon(key string, load func() []byte) []byte {
	appIcons.Lock()
	defer appIcons.Unlock()
	now := time.Now()
	if e, ok := appIcons.entries[key]; ok && now.Sub(e.at) < 5*time.Minute {
		return bytes.Clone(e.data)
	}
	if len(appIcons.entries) >= 128 {
		var oldest string
		var at time.Time
		for k, e := range appIcons.entries {
			if oldest == "" || e.at.Before(at) {
				oldest, at = k, e.at
			}
		}
		delete(appIcons.entries, oldest)
	}
	data := load()
	if !sourceapp.ValidIcon(data) {
		data = nil
	}
	appIcons.entries[key] = iconEntry{bytes.Clone(data), now}
	return data
}

func smallPNG(data []byte) []byte {
	if len(data) == 0 || len(data) > 4<<20 {
		return nil
	}
	cfg, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil || cfg.Width < 1 || cfg.Height < 1 || cfg.Width > 2048 || cfg.Height > 2048 {
		return nil
	}
	img, err := png.Decode(bytes.NewReader(data))
	if err != nil {
		return nil
	}
	dst := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	draw.CatmullRom.Scale(dst, dst.Bounds(), img, img.Bounds(), draw.Over, nil)
	var b bytes.Buffer
	if png.Encode(&b, dst) != nil || !sourceapp.ValidIcon(b.Bytes()) {
		return nil
	}
	return b.Bytes()
}
