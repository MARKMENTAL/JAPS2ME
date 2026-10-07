use anyhow::Result;
use clap::{Parser, Subcommand};
use japs2me::memory_card::transaction::add_directory_atomically;
use japs2me::{CodebreakerSave, ImportFile, MemoryCard};
use std::path::Path;

#[derive(Parser)]
#[command(name = "japs2me", about = "Just A PS2 Memory Card Editor")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    List { card: String },
    Delete { card: String, name: String },
    Add { card: String, save: String },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::List { card } => {
            let mut card = MemoryCard::open(card, false)?;
            for entry in card.entries()? {
                println!("{} {} entries", entry.name, entry.length);
            }
        }
        Command::Delete { card, name } => {
            let mut card = MemoryCard::open(card, true)?;
            card.remove(&name)?;
        }
        Command::Add { card, save } => {
            let save = CodebreakerSave::from_file(Path::new(&save))?;
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
    }
    Ok(())
}
