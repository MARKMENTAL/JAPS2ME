# JAPS2ME

**Just A PS2 Memory Card Editor** is a command-line tool for managing PCSX2 PlayStation 2 memory-card images. It works with card images directly, imports CodeBreaker `.cbs` and Datel MAX Drive `.max` saves, and exports CodeBreaker saves.

## Features

- Browse root saves with game titles, file counts, payload sizes, and card free space.
- Import `.cbs` and `.max` saves with staged validation and interactive progress.
- Delete save directories and export saves as `.cbs` files.
- Read plain and ECC-bearing memory-card images; regenerate ECC when writing.

## Build

You need a Rust toolchain with Cargo installed.

```sh
cargo build --release
```

The executable is created at `target/release/japs2me`.

## Usage

| Command | Purpose |
| --- | --- |
| `japs2me --version` | Print the version (`-V` is also supported). |
| `japs2me list <CARD.ps2>` | List root saves and card free space. |
| `japs2me add <CARD.ps2> <SAVE>` | Import a CodeBreaker `.cbs` or MAX Drive `.max` save. |
| `japs2me delete <CARD.ps2> <SAVE-DIRECTORY>` | Delete a save directory. |
| `japs2me export <CARD.ps2> <SAVE-DIRECTORY> <OUTPUT.cbs>` | Export a save as CodeBreaker. |

Examples:

```sh
target/release/japs2me list Mcd001.ps2
target/release/japs2me add Mcd001.ps2 save-games/PES3allunlocked.max
target/release/japs2me delete Mcd001.ps2 BASLUS-20787SD5
target/release/japs2me export Mcd001.ps2 BESLES-51912PES3OPT pes3.cbs
```

### `list`

- Shows each root save's directory name, game title, regular-file count, and total payload size.
- Reads game titles from `icon.sys`; falls back to the directory name if the metadata is missing or invalid.
- The footer reports free and total allocatable space. Save sizes are payload bytes, not allocated cluster sizes.

### `add`

- Detects `.cbs` and `.max` files by signature and uses the directory name stored in the save.
- Rejects a save if that directory already exists on the card.
- On interactive terminals, progress is shown while the card is copied, the save is written and verified, and the staged image is committed. Progress is hidden when stderr is not a terminal.

### `delete`

Deletes the named root save directory. Unlike imports, deletion currently writes directly to the target card image.

### `export`

Exports a save's files, modes, and timestamps as a CodeBreaker `.cbs` file. The title comes from `icon.sys` when available. Existing output files are not overwritten.

## Safety

- Back up the memory-card image before editing it.
- Keep PCSX2 closed while changing a card.
- Imports are staged and validated before replacing the original image; deletion writes directly to the target.
- Avoid testing writes against your only card image or a supplied fixture.

## Scope compared with mymcplusplus

JAPS2ME focuses on importing, exporting, and basic save management rather than general filesystem access:

- `list` covers root saves; free-space information is included instead of a separate `df` command.
- `add` combines mymcplusplus's `add` and `import` workflows to install a complete `.cbs` or `.max` save.
- `export` supports CodeBreaker `.cbs` only.
- There is no GUI, file extraction, filesystem check, card formatting, mode-flag editing, or ECC override option. JAPS2ME handles plain and ECC-bearing images automatically and regenerates ECC on writes.

For details about the save containers and PS2 memory-card filesystem, see the [technical format guide](docs/FORMATS.md). The [curated save-games collection](save-games/README.md) lists the saves shared in this repository.

## Tests

Run formatting, tests, lint checks, and a release build from the repository root:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## License

JAPS2ME is licensed under the **GNU General Public License, version 3 or (at your option) any later version** (SPDX: `GPL-3.0-or-later`). This project license does not automatically apply to the individual save files in `save-games/`; see that directory's README for contribution and attribution guidance.

## Acknowledgements

Thanks to [Adubbz and the mymcplusplus project](https://github.com/Adubbz/mymcplusplus), a Python PS2 memory-card manager. Its implementation provided a valuable technical reference for understanding the PS2 memory-card filesystem format and behavior.
