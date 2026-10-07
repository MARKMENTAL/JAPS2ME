# Community Save Games

This directory holds a curated collection of PlayStation 2 save containers shared
for community use. Files remain in their original supported container formats:
CodeBreaker `.cbs` and Datel MAX Drive `.max`.

## Collection

| File | Format |
| --- | --- |
| `PES3allunlocked.max` | Datel MAX Drive (`.max`) |
| `PES6allunlocked.max` | Datel MAX Drive (`.max`) |
| `pes6masterleaguemanchesterunited.cbs` | CodeBreaker (`.cbs`) |
| `wwe-smackdown-here-comes-the-pain.20215.cbs` | CodeBreaker (`.cbs`) |
| `wwe-smackdown-shut-your-mouth.27740.cbs` | CodeBreaker (`.cbs`) |

## Using a save

Import a save into a memory-card image with JAPS2ME:

```sh
japs2me add Mcd001.ps2 save-games/PES3allunlocked.max
```

Back up the card first and keep PCSX2 closed while modifying it. `add` stages and
validates the import before replacing the card image. The `.cbs` and `.max` files
contain the save directory names and file data used during import.

## Adding saves

When contributing a save, keep its original `.cbs` or `.max` container and use a
descriptive filename. Include the game, region or product code when known, a brief
description, and where the save came from. Only contribute files you created or have
permission to redistribute. Do not include personal information or credentials in
save data.

The JAPS2ME project is GPL-3.0-or-later, but that license does **not** automatically
apply to save files in this directory. No separate license or redistribution rights
are implied for an individual save; retain any attribution, terms, and permissions
that apply to its source.
