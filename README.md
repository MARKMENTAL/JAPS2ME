# JAPS2ME

**Just A PS2 Memory Card Editor** is a command-line tool for managing PCSX2 PlayStation 2 memory-card images. It can list and delete save directories and import CodeBreaker `.cbs` and Datel MAX Drive `.max` saves.

> **Early development:** Back up your memory-card image before using JAPS2ME. Avoid editing a card while it is open in PCSX2. Although imports are staged and validated before replacing the image, keep an independent backup.

## Features

- List saves as a Unicode table with directory names, game titles, file counts, payload sizes, and free-space footer.
- Delete a save directory.
- Export a save directory as a CodeBreaker `.cbs` file.
- Import a CodeBreaker `.cbs` save, including its files and metadata.
- Import a Datel MAX Drive `.max` save, including LZARI decompression.
- Read and update standard PS2 memory-card images with ECC support.

## Build

You need a Rust toolchain with Cargo installed.

```sh
cargo build --release
```

The executable is created at `target/release/japs2me`.

## Usage

```text
japs2me list <CARD.ps2>
japs2me delete <CARD.ps2> <SAVE-DIRECTORY>
japs2me add <CARD.ps2> <SAVE.cbs|SAVE.max>
japs2me export <CARD.ps2> <SAVE-DIRECTORY> <OUTPUT.cbs>
```

Examples:

```sh
target/release/japs2me list Mcd001.ps2
target/release/japs2me add Mcd001.ps2 my-save.cbs
target/release/japs2me add Mcd001.ps2 PES3allunlocked.max
target/release/japs2me delete Mcd001.ps2 BASLUS-20787SD5
target/release/japs2me export Mcd001.ps2 BESLES-51912PES3OPT pes3.cbs
```

`list` displays saves as a Unicode table with the save directory, game title from `icon.sys` (falling back to the directory name), regular-file count, and total payload size. A footer shows free allocatable space. Game titles are decoded from the Shift-JIS `icon.sys` title field; missing or invalid metadata falls back to the directory name. Listed save sizes are summed file payload bytes, not on-card allocated cluster sizes. Free space is unallocated FAT clusters multiplied by the cluster size. `add` detects CodeBreaker and MAX Drive saves by their file signatures, uses the directory name stored in the save, and reports an error if that save already exists on the card. `export` writes the save directory's files, modes, and timestamps to a CodeBreaker `.cbs` file, using the `icon.sys` title when available, and refuses to overwrite an existing output file.

## Scope compared with mymcplusplus

JAPS2ME focuses on importing, exporting, and basic save management rather than general filesystem access:

- `list` shows root save directories with titles, file counts, and payload sizes; it does not browse arbitrary directories. Free and total space appear in the `list` footer instead of a separate `df` command.
- `add` combines mymcplusplus's `add` and `import` into one step that installs a complete `.cbs` or `.max` save.
- `export` writes a save as CodeBreaker `.cbs` only.
- `delete` removes a named root save directory; arbitrary file or directory management is not supported.
- There is no GUI, file extraction, filesystem check, card formatting, mode-flag editing, or ECC override option. Standard images with or without ECC spare areas are handled automatically, and ECC is regenerated on writes.

For details on the `.cbs` and `.max` containers and the PS2 memory-card filesystem,
see the [technical format guide](docs/FORMATS.md).

## Tests

Run the test suite and lint checks with:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

## License

JAPS2ME is licensed under the **GNU General Public License, version 3 or (at your option) any later version** (SPDX: `GPL-3.0-or-later`).

## Acknowledgements

Thanks to [Adubbz and the mymcplusplus project](https://github.com/Adubbz/mymcplusplus), a Python PS2 memory-card manager. Its implementation provided a valuable technical reference for understanding the PS2 memory-card filesystem format and behavior.
