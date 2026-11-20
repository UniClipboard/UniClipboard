// Command rogue tries a forged valid identity from a different executable.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
)

func main() {
	var identity map[string]any
	if err := json.NewDecoder(os.Stdin).Decode(&identity); err != nil {
		panic(err)
	}
	identity["pid"] = os.Getpid()
	conn, err := connect(identity["profile"].(string), identity["role"].(string))
	if err != nil {
		panic(err)
	}
	defer func() { _ = conn.Close() }()
	if err := json.NewEncoder(conn).Encode(map[string]any{"identity": identity, "command": "__probe"}); err != nil {
		panic(err)
	}
	var response map[string]any
	err = json.NewDecoder(conn).Decode(&response)
	if err == io.EOF || err != nil || response["error"] != nil {
		fmt.Println("rejected")
		return
	}
	fmt.Fprintln(os.Stderr, "forged binary identity accepted")
	os.Exit(1)
}
