#!/usr/bin/env python3
"""Loads a generated PNG through the gdk-pixbuf bundled in an unpacked AppDir (argv[1]) and prints the registered formats and the result
(slice 17c5: the startup error dialog crashed GTK on a host without /usr/share/mime; see docs/architecture/gui-go-linux-appimage-portable.md)."""
import ctypes, os, sys, zlib, struct, subprocess
def png(path):
    raw = b''.join(b'\x00' + b'\xff\x00\x00\xff' * 16 for _ in range(16))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    open(path, 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 16, 16, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))
root = sys.argv[1]
png('/tmp/t.png')
lib = ctypes.CDLL(f'{root}/usr/lib/libgdk_pixbuf-2.0.so.0')
lib.gdk_pixbuf_get_formats.restype = ctypes.c_void_p
lib.gdk_pixbuf_format_get_name.restype = ctypes.c_char_p
lib.gdk_pixbuf_format_get_name.argtypes = [ctypes.c_void_p]
glib = ctypes.CDLL(f'{root}/usr/lib/libglib-2.0.so.0')
glib.g_slist_length.argtypes = [ctypes.c_void_p]; glib.g_slist_nth_data.restype = ctypes.c_void_p; glib.g_slist_nth_data.argtypes = [ctypes.c_void_p, ctypes.c_uint]
l = lib.gdk_pixbuf_get_formats()
print('formats:', [lib.gdk_pixbuf_format_get_name(glib.g_slist_nth_data(l, i)).decode() for i in range(glib.g_slist_length(l))])
lib.gdk_pixbuf_new_from_file.restype = ctypes.c_void_p
err = ctypes.c_void_p()
pb = lib.gdk_pixbuf_new_from_file(b'/tmp/t.png', ctypes.byref(err))
print('png load:', 'OK' if pb else 'FAILED')
rc = 0 if pb else 1
if err.value:
    msg = ctypes.c_char_p.from_address(err.value + 8).value
    print('error:', msg)
sys.exit(rc)
