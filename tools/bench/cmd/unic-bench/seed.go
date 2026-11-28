package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"path/filepath"

	"github.com/uniclipboard/unic/tools/bench/internal/fixture"
)

type seedOutput struct {
	FixtureDir   string              `json:"fixture_dir"`
	ManifestPath string              `json:"manifest_path"`
	Manifest     *fixture.Manifest   `json:"manifest"`
	ProfileHome  string              `json:"profile_home,omitempty"`
	SeedResult   *fixture.SeedResult `json:"seed_result,omitempty"`
}

func cmdSeed(ctx context.Context, args []string, stdout, stderr io.Writer) error {
	fs := flag.NewFlagSet("seed", flag.ContinueOnError)
	fs.SetOutput(stderr)
	presetStr := fs.String("preset", string(fixture.PresetStandard), "corpus preset: empty, standard, or soak")
	outDir := fs.String("out", "", "output directory for generated corpus and fixture-manifest.json")
	profileDir := fs.String("profile", "", "optional isolated UNIC_HOME directory to initialize and seed")
	unicBin := fs.String("unic", "", "path to unic binary (required when --profile is set)")
	unicdBin := fs.String("unicd", "", "path to unicd binary (required when --profile is set)")
	mode := fs.String("mode", "normal", "desktop mode for config.toml: normal or lightweight")
	keepDaemon := fs.Bool("keep-daemon", false, "restart and leave unicd running after seeding")
	asJSON := fs.Bool("json", false, "output JSON summary")
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return &exitError{code: exitUsage, err: err}
	}
	if fs.NArg() != 0 {
		return &exitError{code: exitUsage, err: fmt.Errorf("unexpected positional arguments: %v", fs.Args())}
	}
	if *outDir == "" {
		return &exitError{code: exitUsage, err: errors.New("--out is required")}
	}
	preset, err := fixture.ParsePreset(*presetStr)
	if err != nil {
		return &exitError{code: exitUsage, err: err}
	}

	absOut, err := filepath.Abs(*outDir)
	if err != nil {
		return err
	}
	manifest, err := fixture.Generate(absOut, preset)
	if err != nil {
		return err
	}

	out := seedOutput{
		FixtureDir:   absOut,
		ManifestPath: filepath.Join(absOut, "fixture-manifest.json"),
		Manifest:     manifest,
	}

	if *profileDir != "" {
		if *unicBin == "" || *unicdBin == "" {
			return &exitError{code: exitUsage, err: errors.New("--unic and --unicd are required when --profile is set")}
		}
		prof, err := fixture.InitProfile(ctx, fixture.ProfileOptions{
			HomeDir:     *profileDir,
			UnicBin:     *unicBin,
			UnicdBin:    *unicdBin,
			DesktopMode: *mode,
		})
		if err != nil {
			return err
		}
		seedRes, err := fixture.SeedProfile(ctx, prof, absOut, manifest, *keepDaemon)
		if err != nil {
			_ = prof.StopDaemon(context.Background())
			return err
		}
		out.ProfileHome = prof.HomeDir
		out.SeedResult = seedRes
	}

	if *asJSON {
		enc := json.NewEncoder(stdout)
		enc.SetIndent("", "  ")
		return enc.Encode(out)
	}

	_, _ = fmt.Fprintf(stdout, "Generated %s corpus in %s (texts=%d images=%d files=%d sha256=%s)\n",
		manifest.Preset, absOut, manifest.TextCount, manifest.ImageCount, manifest.FileCount, manifest.ManifestSHA256[:12])
	if out.SeedResult != nil {
		_, _ = fmt.Fprintf(stdout, "Seeded isolated profile %s (history=%d, duration=%dms, daemon_stopped=%t)\n",
			out.ProfileHome, out.SeedResult.HistoryCount, out.SeedResult.DurationMs, out.SeedResult.DaemonStopped)
	}
	return nil
}
