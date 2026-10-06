// Virtual pointer for the headless sway (zwlr_virtual_pointer_v1): moves to an absolute position of the output layout
// and optionally clicks. Compiled inside the container by linux_wayland_run.py; talks only to the compositor named by
// WAYLAND_DISPLAY, never to a host desktop.
//   vpointer            (commands on stdin, see main)
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <wayland-client.h>
#include "wlr-virtual-pointer-unstable-v1-client-protocol.h"

static struct zwlr_virtual_pointer_manager_v1 *manager;
static struct wl_seat *seat;

static void global(void *data, struct wl_registry *registry, uint32_t name, const char *interface, uint32_t version) {
	if (!strcmp(interface, zwlr_virtual_pointer_manager_v1_interface.name))
		manager = wl_registry_bind(registry, name, &zwlr_virtual_pointer_manager_v1_interface, 1);
	else if (!strcmp(interface, "wl_seat") && !seat)
		seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
}
static void global_remove(void *data, struct wl_registry *registry, uint32_t name) {}
static const struct wl_registry_listener listener = {global, global_remove};

static uint32_t now_ms(void) {
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec * 1000u + ts.tv_nsec / 1000000u;
}

// A long-lived virtual pointer, so the seat advertises the pointer capability to every client from the start (a device
// that appears only for one click can lose the race with the client binding wl_pointer). Commands on stdin, one per line:
//   size <layout-width> <layout-height>   absolute motion is mapped onto this extent (the output layout in logical px)
//   move <x> <y>
//   press | release                       the left button
// Each command answers "ok" after the compositor processed it.
int main(void) {
	struct wl_display *display = wl_display_connect(NULL);
	if (!display) { fprintf(stderr, "cannot connect to the Wayland compositor\n"); return 1; }
	struct wl_registry *registry = wl_display_get_registry(display);
	wl_registry_add_listener(registry, &listener, NULL);
	wl_display_roundtrip(display);
	if (!manager) { fprintf(stderr, "compositor lacks zwlr_virtual_pointer_manager_v1\n"); return 1; }
	struct zwlr_virtual_pointer_v1 *pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(manager, seat);
	uint32_t width = 1, height = 1;
	char line[128];
	setvbuf(stdout, NULL, _IOLBF, 0);
	wl_display_roundtrip(display);
	printf("ready\n");
	while (fgets(line, sizeof line, stdin)) {
		unsigned x, y;
		if (sscanf(line, "size %u %u", &width, &height) == 2) {
		} else if (sscanf(line, "move %u %u", &x, &y) == 2) {
			zwlr_virtual_pointer_v1_motion_absolute(pointer, now_ms(), x, y, width, height);
			zwlr_virtual_pointer_v1_frame(pointer);
		} else if (!strncmp(line, "press", 5)) {
			zwlr_virtual_pointer_v1_button(pointer, now_ms(), 0x110 /* BTN_LEFT */, WL_POINTER_BUTTON_STATE_PRESSED);
			zwlr_virtual_pointer_v1_frame(pointer);
		} else if (!strncmp(line, "release", 7)) {
			zwlr_virtual_pointer_v1_button(pointer, now_ms(), 0x110, WL_POINTER_BUTTON_STATE_RELEASED);
			zwlr_virtual_pointer_v1_frame(pointer);
		} else {
			printf("error unknown command\n");
			continue;
		}
		wl_display_roundtrip(display);
		printf("ok\n");
	}
	zwlr_virtual_pointer_v1_destroy(pointer);
	wl_display_roundtrip(display);
	return 0;
}
