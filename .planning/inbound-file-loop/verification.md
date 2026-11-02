# Inbound clipboard loop: text, images, and files

Source: Desktop `9e41d15f7ecf2be4e3bd03327625fd2b4adbbbd5`, Engine `3d3ecb22166f0edab545ce8b4d0b6c71397ea471`.

## Failure modes before implementation

- Host returns no write receipt; Engine cannot suppress the inbound echo.
- Watcher queues a snapshot without its original identity; a later read binds the wrong ownership version.
- External writer replaces clipboard during capture; mixed content must not receive a reliable identity.
- External writer replaces clipboard during write; never return its version as our receipt.
- Identical content copied by another owner must have a different identity.
- Unsupported payload or failed write must not return a successful receipt.
- Multi-file writes must retain native file URL items.
- Legacy clipboard implementations without identities must continue working.

## Verification boundaries

Use real AppKit on a unique private pasteboard for the native writer scenario. Do not change the user's general pasteboard. Verify the host/watcher identity bridge separately. Native GUI and paired-device transfer acceptance remain pending unless explicitly run.

## Verified cause and implementation

The archive's Engine revision matches Desktop's pinned Engine revision. Engine's host adapter calls `HostClipboard::read_with_identity` and `write_with_receipt`. Before this change, `DesktopClipboard` implemented neither method. Their default implementations return no identity. `InMemorySelfWriteLedger::attribute_identity(None)` returns `LocalCapture`.

Desktop now forwards identities through the platform port, watcher event, pending observation, and Engine host API. macOS native writes return the ownership version obtained from `clearContents`, only if it still matches after `writeObjects`. Reads reject a snapshot if ownership changes during capture. Item preparation finishes before clearing the pasteboard.

This follows Apple's documented [pasteboard ownership version contract](https://developer.apple.com/documentation/appkit/nspasteboard/changecount). A private AppKit pasteboard experiment confirmed that a `writeObjects` commit retains the `clearContents` version.

The display metadata representation is still skipped by the native writer. It is not the self-write identity mechanism in this Engine revision. The native scenario succeeds with that representation skipped.

## Results and reproducible commands

Run from the repository root on macOS:

```bash
cargo test -p uc-platform --lib native_file_write_identity_e2e -- --nocapture
cargo test -p uc-platform --lib native_text_image_identity_matrix_e2e -- --nocapture
cargo test -p uc-bootstrap --lib watcher_host_identity_e2e -- --nocapture
cargo test -p uc-bootstrap --lib desktop_clipboard_recovery_process -- --nocapture
cargo check -p uc-bootstrap -p uc-platform --all-targets
cargo fmt --all -- --check
git diff --check
```

All commands passed. The native scenario uses a unique private pasteboard and two real fixture files. It does not alter the user's general pasteboard. The host scenario uses a versioned platform fixture with the real watcher channel and desktop host adapter. It verifies that an ownership change cannot replace a queued observation's identity. The existing recovery scenario runs the real Engine and desktop adapters in a subprocess. The platform's existing 115 checks also passed.

Artifacts: `target/test-artifacts/inbound-file-loop/manifest.json`, `native.txt`, `host.txt`, `diagnostic-events.json`, and build/test logs. The manifest records the base SHA, dirty state, source file hashes, archive hash, and verification boundaries. The initial missing-contract compile failure is retained as `native-red.log`; it is not a runtime reproduction of the network loop.

## Remaining acceptance

- NOT RUN: general system pasteboard notification loop with a running product daemon.
- NOT RUN: paired-device inbound text/image/file transfer and verification of zero return dispatches.
- NOT RUN: native GUI and Windows/Linux runtime acceptance.
- All supported macOS text and image conversions now use the identified native writer. PNG/TIFF retain direct native bytes; other raster formats reuse clipboard-rs conversion to PNG.
- No Engine pin change, installation, or release is part of this fix. Commit and PR publication are separate delivery steps.

## Expanded content scope (before the follow-up implementation)

The second archive confirms a text receive/capture/accepted-return chain at 03:55:27 UTC. It also shows an image receive followed by a new capture and return dispatch attempt at 03:55:40–43 UTC; that dispatch errored, with `accepted=0` and `errored=1`. The image's peer receipt is not proven.

Additional failure modes:

- Single-representation Markdown, CSV, and URL text use an unidentified legacy writer.
- JPEG, GIF, WebP, and BMP conversion can succeed without returning an identity.
- A conversion failure must leave the previous pasteboard ownership and content intact.
- Raster normalization must retain readable image dimensions.

Before changing these paths, extend native private-pasteboard boundary E2E coverage to plain/HTML/RTF/Markdown/CSV/URL text and PNG/TIFF/JPEG/GIF/WebP/BMP images. Use the existing maintained image conversion dependency. Keep one identified native commit path.

## Follow-up result

The expanded native scenario first failed at Markdown because the identified writer did not support its legacy plain-string conversion. The implementation now normalizes Markdown/CSV/URL text to the native string type, and JPEG/GIF/WebP/BMP images to PNG with the existing clipboard-rs decoder and encoder. It performs conversion before taking pasteboard ownership. It removes the successful-but-unidentified write fallback.

The native private-pasteboard matrix passed for all 12 content formats. It verified readback bytes for text, readable dimensions for images, matching write receipts, new ownership versions, and preservation of ownership on image conversion failure. The original native file scenario also passed. The watcher/host identity scenario, all-target compilation, formatting, and diff checks passed on this updated source. The earlier 115-check platform run and Engine recovery run predate the format normalization change.

Evidence from the new archive is in `target/test-artifacts/inbound-file-loop/text-image-diagnostic-events.json`. The expanded native scenario writes `text-image-matrix.txt`. Its failing and passing executions are in `text-image-red.log` and `text-image-green.log`. The manifest records updated source hashes and separates current verification from earlier verification.
