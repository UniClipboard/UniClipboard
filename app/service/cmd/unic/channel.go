package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	api "github.com/uniclipboard/unic/internal/localapi"
	"golang.org/x/term"
)

func runChannelPair(ctx context.Context, c *api.Client, args []string) error {
	if len(args) < 1 || len(args) > 2 || (len(args) == 2 && args[1] != "--resume") {
		return errors.New("usage: unic channel pair INSTANCE [--resume]")
	}
	req := api.ChannelPairRequest{BackendID: args[0], Resume: len(args) == 2}
	if !req.Resume {
		fd := int(os.Stdin.Fd())
		var raw []byte
		var err error
		if term.IsTerminal(fd) {
			fmt.Fprint(os.Stderr, "Enter the provider pairing code. Pairing enables clipboard sync with the provider: ")
			raw, err = term.ReadPassword(fd)
			fmt.Fprintln(os.Stderr)
		} else {
			raw, err = io.ReadAll(io.LimitReader(os.Stdin, 9))
		}
		req.Code = strings.TrimSpace(string(raw))
		clear(raw)
		if err != nil || len(req.Code) != 6 {
			return errors.New("provide the six-digit pairing code on standard input")
		}
	}
	out, err := c.PairChannel(ctx, req)
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(out)
}
