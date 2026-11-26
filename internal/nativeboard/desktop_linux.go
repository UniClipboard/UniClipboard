package nativeboard

import (
	"golang.design/x/clipboard"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/waylandwire"
)

var desktopFormats = []desktopFormat{
	{mime: clip.Text, format: clipboard.FmtText},
	{mime: clip.HTML, format: clipboard.Register(clip.HTML)},
	{mime: clip.PNG, format: clipboard.FmtImage},
}

func checkDesktopSession() error {
	path := waylandwire.SocketPath()
	if path == "" {
		conn := sourceX11()
		if conn == nil {
			return ErrDesktopClipboardUnsupported
		}
		conn.Close()
		return nil
	}
	conn, err := waylandwire.Dial(path)
	if err != nil {
		return ErrDesktopClipboardUnsupported
	}
	defer conn.Close()
	globals, err := conn.DiscoverGlobals()
	if err != nil || globals["wl_seat"].Version == 0 {
		return ErrDesktopClipboardUnsupported
	}
	if globals["ext_data_control_manager_v1"].Version == 0 && globals["zwlr_data_control_manager_v1"].Version < 2 {
		return ErrDesktopClipboardUnsupported
	}
	return nil
}

// X11 owners can publish new content from the same window, and the Wayland
// backend does not expose a selection serial. Use the full observation there.
func (*Board) ClipboardGeneration() string { return "" }

func platformClipboard() *Board {
	if waylandwire.SocketPath() == "" {
		return &Board{read: readX11Clipboard, write: writeX11Clipboard}
	}
	return nil
}
