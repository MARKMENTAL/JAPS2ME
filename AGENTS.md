# Agent and Contributor Guide

## Project overview

JAPS2ME (Just A PS2 Memory Card Editor) is a Rust CLI and library for managing PCSX2 PlayStation 2 memory-card images. Current CLI commands:

- `list <CARD.ps2>` — list saves as a Unicode table with game titles, file counts, payload sizes, and free space.
- `delete <CARD.ps2> <SAVE-DIRECTORY>` — remove a save directory.
- `add <CARD.ps2> <SAVE.cbs|SAVE.max>` — import a Codebreaker or Datel MAX Drive save.

The project is licensed under GPL-3.0-or-later. `mymcplusplus` is a technical reference for PS2 memory-card filesystem behavior and save formats; see its attribution in `README.md`.

## Repository map

- `src/main.rs` — Clap CLI and command dispatch.
- `src/commands/list.rs` — Unicode table rendering and byte-size formatting for `list`.
- `src/memory_card/icon_sys.rs` — `icon.sys` title parsing with Shift-JIS decoding.
- `src/lib.rs` — crate modules and public exports.
- `src/save.rs` — shared imported-save model and format detection.
- `src/codebreaker/format.rs` — Codebreaker RC4/zlib parser.
- `src/max_drive.rs` — MAX Drive header/CRC parser, LZARI decoder, and file-record parser.
- `src/memory_card/card.rs` — memory-card access, directory operations, allocation, FAT chains, and save insertion/removal.
- `src/memory_card/superblock.rs` — superblock parsing and card geometry.
- `src/memory_card/directory.rs` — 512-byte directory-entry encoding and mode bits.
- `src/memory_card/fat.rs` — FAT marker constants and helpers.
- `src/memory_card/ecc.rs` — PS2 page spare-area ECC generation.
- `src/memory_card/transaction.rs` — staged, validated, copy-on-write save imports.
- `PES3allunlocked.max` — supplied MAX Drive test fixture used by `include_bytes!` tests; keep it available at the repository root when running the test suite.
- `mymcplusplus-reference/` — cloned Python reference implementation; do not modify it as part of normal JAPS2ME work.

## Build and checks

Run these from the repository root:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

After changing code, rebuild the executable being invoked. For example, `cargo build` refreshes `target/debug/japs2me`; `cargo build --release` refreshes `target/release/japs2me`. A stale binary can continue exhibiting behavior fixed in the source.

## Memory-card safety

- Never use the user's only memory-card image as a test target. Do not test writes against `Mcd001.ps2`, `Mcd001edited.ps2`, or another supplied `.ps2` fixture.
- Make a temporary copy under `/tmp/opencode/` for integration tests. The add command already stages imports on a temporary image and replaces the target only after validation; delete currently writes directly to its target.
- Keep PCSX2 closed while changing a card image, and recommend an independent backup before destructive operations.
- For external compatibility checks, use mymcplusplus read-only commands such as `check` and `ls` against the temporary image. `check` should report no filesystem errors; verify imported file bytes against the source save when possible.

## Filesystem and directory invariants

- PS2 memory-card directory entries are 512 bytes. Directory `length` is an entry count; regular-file `length` is a byte count.
- FAT links use the allocated high bit. A file or directory may occupy multiple clusters; every link and tail marker must be updated when allocating or freeing a chain.
- Logical allocatable clusters are located after the metadata region. FAT and indirect FAT clusters are addressed as raw card clusters; file and directory clusters are addressed relative to `allocatable_cluster_offset`.
- The root directory can span multiple FAT clusters. Root-slot lookup, listing, insertion, and deletion must traverse the full chain rather than assume a single cluster.
- A save directory's `.` and `..` entries have length zero. The save-directory entry in its parent (normally the root) stores the number of directory entries, including `.` and `..`.
- When changing page data on ECC-bearing cards, regenerate the page's spare-area ECC. Do not write only page data while leaving stale ECC bytes.
- MAX and Codebreaker inputs should be fully validated before the staged card replaces the original. Preserve file names, payload bytes, supported modes, and timestamps as appropriate for their source format.
- `list` summaries count regular files only and sum their stored byte lengths; directory entries such as `.` and `..` are excluded. Free space is unallocated FAT clusters times the cluster size and is distinct from on-card allocated space.

## Save-format details

- Codebreaker files begin with `CFU\0`. Their body is RC4-decrypted using the fixed Codebreaker state, then zlib-decompressed. Header and per-file lengths must be bounds-checked.
- MAX Drive files begin with `Ps2PowerSave` and have a 92-byte header. The CRC is computed over the header with its CRC field zeroed followed by the compressed payload. The LZARI output contains `dirlen` records: little-endian data length, 32-byte name, file bytes, and the format's alignment padding.
- The supplied `PES3allunlocked.max` fixture should parse as directory `BESLES-51912PES3OPT`, with `icon.sys` (964 bytes), `icon_view` (33,400 bytes), and `BESLES-51912PES3OPT` (773,120 bytes). Tests use CRCs of the decoded files to catch LZARI regressions.

## Change guidelines

- Keep parsing separate from memory-card mutation; both save formats should produce the shared `SaveImport` model.
- Prefer focused tests for parser boundaries, FAT/directory invariants, and failed-import rollback.
- Do not weaken MAX CRC, decompressed-size, or record-bound checks to accept malformed input without a documented compatibility reason and a regression test.
- Update `README.md` when user-facing commands, supported formats, build steps, or safety behavior change.
