package lua

// Library names. The upstream OpenLibs/luaLibs wiring is removed: this fork
// never opens a library by itself; the sandbox builds the environment.
const (
	TabLibName    = "table"
	StringLibName = "string"
	MathLibName   = "math"
)
