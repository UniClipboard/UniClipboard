package main

import (
	"encoding/binary"
	"unsafe"

	"golang.org/x/sys/windows"
)

// Use the default hardware adapter only. Never promote WARP or a model name
// to evidence that the WebView can render full effects at interactive rates.
func probeVisualCapabilities() evidenceClass {
	kernel := windows.NewLazySystemDLL("kernel32.dll")
	var memory struct {
		Length, Load                                                                     uint32
		TotalPhys, AvailPhys, TotalPage, AvailPage, TotalVirtual, AvailVirtual, Extended uint64
	}
	memory.Length = uint32(unsafe.Sizeof(memory))
	if ok, _, _ := kernel.NewProc("GlobalMemoryStatusEx").Call(uintptr(unsafe.Pointer(&memory))); ok == 0 {
		memory.TotalPhys = 0
	}
	var size uint32
	logical := kernel.NewProc("GetLogicalProcessorInformationEx")
	logical.Call(0, 0, uintptr(unsafe.Pointer(&size))) // RelationProcessorCore
	var cores uint64
	if size > 0 && size <= 1024*1024 {
		buffer := make([]byte, size)
		if ok, _, _ := logical.Call(0, uintptr(unsafe.Pointer(&buffer[0])), uintptr(unsafe.Pointer(&size))); ok != 0 {
			for offset := uint32(0); offset+8 <= size; {
				length := binary.LittleEndian.Uint32(buffer[offset+4:])
				if length < 8 || length > size-offset {
					cores = 0
					break
				}
				if binary.LittleEndian.Uint32(buffer[offset:]) == 0 {
					cores++
				}
				offset += length
			}
		}
	}
	graphics := evidenceUnknown
	create := windows.NewLazySystemDLL("d3d11.dll").NewProc("D3D11CreateDevice")
	levels := []uint32{0xc100, 0xc000, 0xb100, 0xb000, 0xa100, 0xa000, 0x9300, 0x9200, 0x9100}
	var level uint32
	if create.Find() == nil {
		call := func(values []uint32) uint32 {
			hr, _, _ := create.Call(0, 1, 0, 0, uintptr(unsafe.Pointer(&values[0])), uintptr(len(values)), 7, 0, uintptr(unsafe.Pointer(&level)), 0)
			return uint32(hr)
		}
		hr := call(levels)
		if hr == 0x80070057 {
			hr = call(levels[3:])
		}
		if int32(hr) >= 0 {
			if level >= 0xc000 {
				graphics = evidenceCapable
			} else {
				graphics = evidenceConstrained
			}
		}
	}
	return classifyVisualCapabilities(cores, memory.TotalPhys, graphics)
}
