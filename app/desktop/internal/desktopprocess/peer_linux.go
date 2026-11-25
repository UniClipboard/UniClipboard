package desktopprocess

import (
	"fmt"
	"os"

	"golang.org/x/sys/unix"
)

func processExecutable(pid int) (string, error) { return os.Readlink(fmt.Sprintf("/proc/%d/exe", pid)) }

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
