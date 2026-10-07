use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use japs2me::codebreaker::format::encode as encode_codebreaker;
use japs2me::memory_card::transaction::add_directory_atomically;
use japs2me::{format_save_table, ImportFile, MemoryCard, SaveImport};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "japs2me", version, about = "Just A PS2 Memory Card Editor")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    List {
        card: String,
    },
    Delete {
        card: String,
        name: String,
    },
    Add {
        card: String,
        save: String,
    },
    Export {
        card: String,
        name: String,
        output: String,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::List { card } => {
            let mut card = MemoryCard::open(card, false)?;
            let summaries = card.save_summaries()?;
            let free_bytes = card.available_bytes()?;
            let total_bytes = card.total_bytes()?;
            println!("{}", format_save_table(&summaries, free_bytes, total_bytes));
        }
        Command::Delete { card, name } => {
            let mut card = MemoryCard::open(card, true)?;
            card.remove(&name)?;
        }
        Command::Add { card, save } => {
            let save = SaveImport::from_file(Path::new(&save))?;
            let files = save
                .files
                .iter()
                .map(|file| ImportFile {
                    name: file.name.clone(),
                    data: file.data.clone(),
                    mode: file.mode,
                    created: file.created,
                    modified: file.modified,
                })
                .collect::<Vec<_>>();
            add_directory_atomically(
                &card,
                &save.directory,
                save.directory_mode,
                save.created,
                save.modified,
                &files,
            )?;
        }
        Command::Export { card, name, output } => {
            let mut card = MemoryCard::open(card, false)?;
            let save = card.export_save(&name)?;
            let data = encode_codebreaker(&save)?;
            write_output_atomically(&output, &data)?;
        }
    }
    Ok(())
}

fn write_output_atomically(output: &str, data: &[u8]) -> Result<()> {
    let output = PathBuf::from(output);
    if output.symlink_metadata().is_ok() {
        anyhow::bail!("output already exists: {}", output.display());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let mut temp = tempfile::NamedTempFile::new_in(parent.unwrap_or_else(|| Path::new(".")))
        .context("creating temporary export file")?;
    temp.write_all(data)
        .context("writing temporary export file")?;
    temp.persist(&output)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}
