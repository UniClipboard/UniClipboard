package main

/*
#cgo CFLAGS: -x objective-c -fobjc-arc
#cgo LDFLAGS: -framework Metal -framework Foundation
#include <stdint.h>
#include <sys/sysctl.h>
#import <Metal/Metal.h>

static uint64_t uc_effects_sysctl(const char *name) {
    uint64_t value = 0;
    size_t size = sizeof(value);
    if (sysctlbyname(name, &value, &size, NULL, 0) != 0) return 0;
    return value;
}

static uint64_t uc_effects_cores(void) { return uc_effects_sysctl("hw.physicalcpu"); }
static uint64_t uc_effects_memory(void) { return uc_effects_sysctl("hw.memsize"); }

static int uc_effects_metal(void) {
    @autoreleasepool {
        NSArray<id<MTLDevice>> *devices = MTLCopyAllDevices();
        if (devices.count == 0) return 0;
        for (id<MTLDevice> device in devices) {
            if (![device supportsFamily:MTLGPUFamilyMac2]) {
                return 1;
            }
        }
        return 2;
    }
}
*/
import "C"

func probeVisualCapabilities() evidenceClass {
	graphics := evidenceUnknown
	switch C.uc_effects_metal() {
	case 1:
		graphics = evidenceConstrained
	case 2:
		graphics = evidenceCapable
	}
	return classifyVisualCapabilities(uint64(C.uc_effects_cores()), uint64(C.uc_effects_memory()), graphics)
}
