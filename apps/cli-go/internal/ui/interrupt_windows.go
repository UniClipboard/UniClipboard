package ui

import "os"

// raiseInterrupt exits with STATUS_CONTROL_C_EXIT, as a Ctrl-C-terminated
// console process does.
func raiseInterrupt() { os.Exit(0xC000013A) }
