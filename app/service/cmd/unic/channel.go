package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"

	"golang.org/x/term"

	api "github.com/uniclipboard/unic/internal/localapi"
)

func runChannelPair(ctx context.Context, c *api.Client, args []string) error {
	var resume bool
	asJSON := globalJSON
	var pos []string
	for _, a := range args {
		switch a {
		case "--resume":
			resume = true
		case "--json":
			asJSON = true
		default:
			if strings.HasPrefix(a, "-") {
				return usageError("usage: unic channel pair <instance> [--resume] [--json]")
			}
			pos = append(pos, a)
		}
	}
	if len(pos) != 1 {
		return usageError("usage: unic channel pair <instance> [--resume] [--json]")
	}
	req := api.ChannelPairRequest{BackendID: pos[0], Resume: resume}
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
	if asJSON || !outTTY {
		return json.NewEncoder(os.Stdout).Encode(out)
	}
	fmt.Printf("%s paired channel %s (%s)\n", style(outTTY, sOK, "✓"), out.Name, out.ID)
	return nil
}
