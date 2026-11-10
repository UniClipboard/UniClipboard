// Command snapshot injects a test snapshot through the production local API.
// It is not a distributed unic command or a transport implementation.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

func main() {
	if err := execute(); err != nil {
		fmt.Fprintln(os.Stderr, "snapshot injection failed")
		os.Exit(1)
	}
}

func execute() error {
	if len(os.Args) != 2 {
		return fmt.Errorf("expected a snapshot fixture")
	}
	b, err := os.ReadFile(os.Args[1])
	if err != nil {
		return err
	}
	var input api.SubmitInput
	if err := json.Unmarshal(b, &input); err != nil {
		return err
	}
	socket, err := paths.Socket()
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	out, err := api.NewClient(socket).Submit(ctx, input)
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(out)
}
