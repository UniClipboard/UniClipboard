# Directory content contract

Status: Accepted specification for implementation under 0024. Implementation is present; acceptance is recorded separately in the [execution plan](../exec/0024-directory-copy-and-send.md).

## Selection and preparation

A directory copy imports a deferred source reference as a file part with MIME `application/vnd.unic.directory-reference`. Its declared size is zero and its host-owned source validates the selected directory identity without enumerating it. A mixed selection retains all top-level references. References remain private database metadata and are never transferred. Their history kind is `directory_reference`; they are not immutable byte snapshots.

Explicit submission prepares a new immutable content object before processing and commit. Automatic submission does so only when global sync, file permission, automatic sync and `sync.directories.automatic` permit it. Otherwise it commits the reference locally with no dispatch. Preparation never runs in the clipboard polling thread. All targets of one operation share the resulting snapshot. Retrying the same admitted request returns its original result, without another scan.

A `DirectorySource` is a host adapter whose `ReadDirectory` returns bounded entries and an operation-local validation callback. Concurrent sends never share mutable scan state. Engine streams each member into owned storage and calculates SHA-256. A changed source returns `source_changed`; unsupported entries reject the entire selection. This detects observed changes but does not guarantee a filesystem-atomic view during concurrent writes.

## Manifest and payloads

A completed directory content has regular-file parts and one representation with MIME `application/vnd.unic.directory+json`. This representation is structured content metadata, not a text alternative or an archive. The JSON manifest has ordered roots `{name,directory}` and entries `{root,path,directory,file,size,sha256}`. `root` is a zero-based root index; `file` is a zero-based file-payload index. Directory entries have no payload. A directory root has an explicit empty-path directory entry. A regular-file root has one empty-path file entry. All file payloads occur exactly once in the manifest. SHA-256 is lowercase hexadecimal over the exact file bytes.

Limits: at most 128 roots, 4096 entries, 2048 file payloads, 64 path components, 4096 UTF-8 bytes per relative path, 255 UTF-8 bytes per component, and 512 KiB encoded manifest. Existing per-file limits and the 4 GiB total protocol cap apply. Manifest bytes count toward total and storage budgets. Existing tighter runtime limits also apply. Zero-byte trees still consume manifest bytes and entry budget. Automatic file thresholds apply to the sum of file bytes of the whole directory selection.

Paths use `/`. Reject absolute paths, drive names, backslashes, empty interior components, `.` and `..`, control characters, invalid UTF-8, Windows-reserved punctuation/names, trailing spaces/dots, duplicate normalized paths, and file/directory conflicts. Compare names using Unicode NFC and case folding for portable collisions. Reject overlapping selected roots and duplicate roots. Equal top-level names use distinct internal root containers and remain separate clipboard items. Do not silently rename descendants.

Include hidden files and empty directories. Ignore no entries implicitly. Reject symlinks, junctions, and special files rather than following or omitting them. Copy hard links as independent regular files. Preserve file bytes and tree shape; do not promise ownership, ACL, executable mode, timestamps, extended attributes, or resource forks.

## Policy and receive

The manifest and all its file payloads form one atomic content batch under file policy, including trees with no files. Text policy cannot strip the manifest. A deferred reference cannot enter any backend. Wire and plugin RPC version remains 1. The manifest travels as an existing representation and files as existing file streams; no protocol compatibility branch is added.

Validate the bounded manifest, payload bijection, lengths, hashes and paths before publication. Reject the entire batch on any failure. Receive staging and managed materialization stay under an opened owned root. No partial history or Received item becomes available. Never merge a tree into an existing destination.

The default cache retains payloads with its existing ledger. Materialized trees are lease-owned views, rebuilt from retained payloads as required. They must not create uncharged byte copies. Source and custom received trees remain user-managed. Cleanup must never follow links, delete source data, delete published custom content, or remove content while a clipboard/send holder needs it.

## Copy actions and application

Use a local clipboard change marker independently of canonical network content identity. Directory polling never walks descendants. A platform that lacks a reliable marker must report its repeated-copy limitation; unchanged name/size hashes do not prove another copy action. Test clipboards provide an explicit deterministic copy marker.

Apply top-level local tree paths through the existing ticket, lease, stale-observation and echo protocol. A late receive cannot replace a newer local copy. Source-reference restore does not trigger implicit directory preparation or transfer. Explicit history send prepares source references anew and resends immutable snapshots without reading the original source.

## Acceptance

Use real paired service processes and deterministic fixture trees. Record no-work evidence separately from absence of sends. Retain inventories, empty directories, per-file SHA-256, source/binary identities, case results, replay instructions and cleanup outcomes. Native clipboard and cross-application paste remain separate platform acceptance.

## Platform limits

macOS uses the pasteboard change count and Windows uses the clipboard sequence number. These paths are compile-checked separately from native acceptance. The current Linux mygo adapter has no reliable generation marker: it supports explicit send and changed selections, but cannot detect another copy of an identical selection. Desktop settings disclose this limit. Native clipboard and cross-application paste remain NOT_RUN until separate authorized acceptance.

Directory arguments require paired-device `--to` sending. Temporary transfer codes and links do not claim directory support. Ownership, ACLs, timestamps, extended attributes, and resource forks are not transferred.

A newer local copy supersedes unfinished automatic directory preparation. Explicit sends have independent lifetimes. Submission cancellation applies only to the Engine-owned submission context; short-lived adapter RPC contexts must never cancel queued history-restore sends.
