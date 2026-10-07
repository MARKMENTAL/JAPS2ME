# PS2 Save and Memory-Card Formats

This guide describes the formats JAPS2ME currently reads or writes. Some fields are
reserved, undocumented, or vary between producers; those are called out rather than
assigned meanings. The MAX Drive and memory-card notes were informed by the
[mymcplusplus project](https://github.com/Adubbz/mymcplusplus), which is also credited
in the main README.

## CodeBreaker `.cbs`

A CodeBreaker file has a fixed header followed by an encrypted, compressed record body.
Integers are little-endian. JAPS2ME identifies the format by its four-byte `CFU\0`
signature.

### Header

| Offset | Size | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Magic: `CFU\0` |
| `0x04` | 4 | Reserved/unknown |
| `0x08` | 4 | Header length in bytes; at least 124 |
| `0x0C` | 4 | Decompressed body length |
| `0x10` | 4 | Stored length; JAPS2ME writes the complete file length |
| `0x14` | 32 | Save-directory name, NUL-terminated or zero-padded |
| `0x34` | 8 | Directory creation timestamp, stored as PS2 time bytes |
| `0x3C` | 8 | Directory modification timestamp |
| `0x44` | 8 | Reserved/unknown |
| `0x4C` | 4 | Directory mode |
| `0x50` | 12 | Reserved/unknown |
| `0x5C` | Variable | Display title, extending to the end of the header |

The title begins at byte 92. The smallest header is 124 bytes, leaving at least 32
bytes for a title. Longer titles require a longer header. In the current encoder the
title is written as UTF-8 and the header is extended when needed. The parser accepts
the stored-length field as either the complete file length or the body length, for
compatibility with existing files.

### Body

The body is zlib-compressed and then RC4-encrypted with the fixed CodeBreaker state
used by the format. Reading reverses that order: RC4-decrypt, then zlib-decompress.
The decompressed stream is a sequence of file records, with no padding between them:

| Record offset | Size | Meaning |
| --- | ---: | --- |
| `0x00` | 8 | File creation timestamp |
| `0x08` | 8 | File modification timestamp |
| `0x10` | 4 | File data length in bytes |
| `0x14` | 2 | PS2 directory-entry mode |
| `0x16` | 2 | Reserved |
| `0x18` | 8 | Reserved/unknown fields |
| `0x20` | 32 | File name, NUL-terminated or zero-padded |
| `0x40` | Variable | File data, exactly the length specified above |

The next record begins immediately after the file data. The body ends after the final
record. JAPS2ME bounds-checks record headers, names, and data lengths and caps the
decompressed body at 64 MiB.

## Datel MAX Drive `.max`

MAX Drive files have a 92-byte header followed by an LZARI-compressed payload. The
header begins with the 12-byte `Ps2PowerSave` signature. Integers are little-endian.

| Offset | Size | Meaning |
| --- | ---: | --- |
| `0x00` | 12 | Magic: `Ps2PowerSave` |
| `0x0C` | 4 | CRC-32 |
| `0x10` | 32 | Save-directory name |
| `0x30` | 32 | Display title |
| `0x50` | 4 | Compressed-length field; producer conventions vary |
| `0x54` | 4 | File count |
| `0x58` | 4 | Decompressed body length |
| `0x5C` | — | Compressed payload begins |

The CRC is calculated over the full 92-byte header with bytes `0x0C..0x10` zeroed,
followed by the compressed payload. It is checked before decompression. The length
field commonly includes four bytes beyond the compressed payload; some files instead
store the uncompressed length there, in which case JAPS2ME treats the payload as
extending to end-of-file.

After LZARI decompression, each file record begins with a 36-byte header:

| Record offset | Size | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | File data length in bytes |
| `0x04` | 32 | File name |
| `0x24` | Variable | File data, followed by format-specific alignment padding |

After a record's data, the next record offset is
`round_up(file_end + 8, 16) - 8`, where `file_end` is the offset immediately after
the file data. This is the format's 16-byte alignment rule with an 8-byte offset
adjustment.
The decompressed body must contain exactly the declared number of records and no
unexpected trailing bytes. MAX files do not carry the per-file timestamps and modes
found in a CodeBreaker file; JAPS2ME assigns supported default values when importing.
The decompressed-size limit is 64 MiB and the file-count limit is 4096.

## Sony PS2 memory-card filesystem

A PS2 memory card is **not a standard FAT filesystem**. It has Sony's own superblock,
a two-level indirect FAT, fixed-size directory entries, and optional ECC spare bytes
attached to each page. “FAT-like” describes the cluster-chain allocation approach,
not compatibility with FAT12/16/32 tools.

### Image geometry and superblock

A standard data page is 512 bytes, and a standard cluster is two pages (1024 bytes).
An erase block is normally 16 pages. ECC-bearing images interleave spare data after
each page; for a 512-byte page this implementation expects 16 spare bytes, making a
raw page 528 bytes. The spare area is outside the filesystem's file payloads.

The first 512 data bytes contain the superblock. Important fields include:

| Offset | Meaning |
| --- | --- |
| `0x00` | Magic: `Sony PS2 Memory Card Format ` (28 bytes) |
| `0x1C` | Format version string (12 bytes) |
| `0x28` | Page size (`u16`) |
| `0x2A` | Pages per cluster (`u16`) |
| `0x2C` | Pages per erase block (`u16`) |
| `0x30` | Total clusters on card (`u32`) |
| `0x34` | First allocatable cluster (`u32`) |
| `0x38` | End of allocatable range, exclusive (`u32`) |
| `0x3C` | Root-directory FAT cluster (`u32`; normally zero) |
| `0x40`, `0x44` | Good-block values used for card geometry/ECC |
| `0x50` | Indirect-FAT cluster list (up to 32 `u32` entries) |
| `0xD0` | Bad erase-block list (up to 32 `u32` entries) |

The superblock and FAT metadata are addressed as raw card clusters. Directory and file
cluster numbers are logical numbers relative to the first allocatable cluster; the
image offset is found by adding the superblock's allocatable-cluster offset.

### FAT and allocation chains

The superblock points to indirect FAT clusters. Each indirect FAT entry points to a
FAT cluster, and each FAT entry describes one allocatable cluster. A chain link sets
the high bit (`0x80000000`) and stores the next logical cluster number in the lower
bits. The all-ones value `0xFFFFFFFF` marks the end of an allocated chain; an
unallocated entry commonly has the high bit clear, such as `0x7FFFFFFF`.

Files and directories can span multiple clusters. Readers follow links until the
end-of-chain marker, detect loops and invalid links, and use the directory entry's
length to distinguish payload bytes from cluster padding. Free space is the count of
unallocated FAT clusters multiplied by the cluster size; it is not the sum of unused
bytes in allocated clusters.

### Directory entries

Each directory entry occupies 512 bytes. Relevant fields are:

| Offset | Size | Meaning |
| --- | ---: | --- |
| `0x00` | 2 | Mode and entry-type flags |
| `0x04` | 4 | Length: entry count for a directory, bytes for a regular file |
| `0x08` | 8 | Creation timestamp |
| `0x10` | 4 | First cluster |
| `0x14` | 4 | Parent-directory entry index |
| `0x18` | 8 | Modification timestamp |
| `0x20` | 4 | Attribute field |
| `0x40` | 448 | Name, normally NUL-terminated |

Common mode bits are read (`0x0001`), write (`0x0002`), execute (`0x0004`), file
(`0x0010`), directory (`0x0020`), and exists (`0x8000`).

Directories begin with `.` and `..`. Those entries have length zero. A save's entry
in its parent records the save directory's entry count, including `.` and `..`; the
root has no parent, so its own `.` entry carries the root entry count. The root
directory itself may occupy a multi-cluster FAT chain.

### ECC and JAPS2ME behavior

JAPS2ME reads plain images and images with ECC spare areas. When it writes data pages
to an ECC-bearing image, it regenerates the affected page's spare-area ECC. It does
not currently offer an ECC-ignore option or validate the existing spare-area ECC on
reads.

Save imports are staged on a temporary image, re-opened, and checked before replacing
the original card. Deletion currently writes directly to its target, so use a backup
and keep PCSX2 closed before destructive operations. JAPS2ME's save model is flat:
export and deletion reject nested directories rather than silently treating them as
ordinary files.

## References

- [mymcplusplus](https://github.com/Adubbz/mymcplusplus), a technical reference for
  PS2 memory-card filesystem behavior and save formats.
- The corresponding JAPS2ME implementations: `src/codebreaker/format.rs`,
  `src/max_drive.rs`, and `src/memory_card/`.
