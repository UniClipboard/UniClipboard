package desktopprocess

import (
	"errors"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
	"golang.org/x/sys/unix"
)

var procPath struct {
	once sync.Once
	call func(int32, unsafe.Pointer, uint32) int32
	err  error
}

func processExecutable(pid int) (string, error) {
	procPath.once.Do(func() {
		handle, err := purego.Dlopen("/usr/lib/libproc.dylib", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			procPath.err = err
			return
		}
		symbol, err := purego.Dlsym(handle, "proc_pidpath")
		if err != nil {
			procPath.err = err
			return
		}
		purego.RegisterFunc(&procPath.call, symbol)
	})
	if procPath.err != nil {
		return "", procPath.err
	}
	path := make([]byte, 4096)
	if n := procPath.call(int32(pid), unsafe.Pointer(&path[0]), uint32(len(path))); n <= 0 {
		return "", errors.New("desktop IPC peer executable unavailable")
	}
	for i, b := range path {
		if b == 0 {
			return string(path[:i]), nil
		}
	}
	return "", errors.New("desktop IPC peer executable path too long")
}

func unixFileStamp(st *unix.Stat_t) (fileStamp, bool) {
	return fileStamp{
		dev:       uint64(st.Dev),
		ino:       uint64(st.Ino),
		size:      st.Size,
		mtimeSec:  int64(st.Mtim.Sec),
		mtimeNsec: int64(st.Mtim.Nsec),
		ctimeSec:  int64(st.Ctim.Sec),
		ctimeNsec: int64(st.Ctim.Nsec),
	}, true
}
