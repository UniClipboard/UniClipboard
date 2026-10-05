package ui

// raiseInterrupt is a no-op on Windows: the console delivers Ctrl-C to the
// process group itself, and the caller reports the interrupted read.
func raiseInterrupt() {}
