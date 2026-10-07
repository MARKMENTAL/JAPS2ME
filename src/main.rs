use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use japs2me::codebreaker::format::encode as encode_codebreaker;
use japs2me::memory_card::transaction::{add_directory_atomically_with_progress, AddProgress};
use japs2me::{format_save_table, ImportFile, MemoryCard, SaveImport};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddStage {
    Copying,
    Writing,
    Verifying,
    Committing,
}

struct AddProgressReporter {
    bar: ProgressBar,
    stage: Option<AddStage>,
    interactive: bool,
}

impl AddProgressReporter {
    fn new() -> Self {
        let interactive = std::io::stderr().is_terminal();
        Self {
            bar: if interactive {
                ProgressBar::new(1)
            } else {
                ProgressBar::hidden()
            },
            stage: None,
            interactive,
        }
    }

    fn update(&mut self, progress: AddProgress) {
        match progress {
            AddProgress::CopyingCard {
                bytes_copied,
                total_bytes,
            } => self.update_bar(
                AddStage::Copying,
                "Copying card",
                bytes_copied,
                total_bytes,
                true,
            ),
            AddProgress::WritingSave {
                bytes_written,
                total_bytes,
            } => self.update_bar(
                AddStage::Writing,
                "Writing save",
                bytes_written,
                total_bytes,
                true,
            ),
            AddProgress::VerifyingSave {
                files_verified,
                total_files,
            } => self.update_bar(
                AddStage::Verifying,
                "Verifying save",
                files_verified as u64,
                total_files as u64,
                false,
            ),
            AddProgress::Committing => {
                self.begin_stage(AddStage::Committing);
                if self.interactive {
                    self.bar = ProgressBar::new_spinner();
                    self.bar.set_style(
                        ProgressStyle::with_template("{spinner:.green} {msg}")
                            .expect("valid commit progress template"),
                    );
                    self.bar.enable_steady_tick(Duration::from_millis(100));
                }
                self.bar.set_message("Committing card");
            }
        }
    }

    fn update_bar(
        &mut self,
        stage: AddStage,
        message: &str,
        position: u64,
        length: u64,
        is_bytes: bool,
    ) {
        if self.stage != Some(stage) {
            self.begin_stage(stage);
            if self.interactive {
                self.bar = ProgressBar::new(length);
                let template = if is_bytes {
                    "{msg:>18} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
                } else {
                    "{msg:>18} [{bar:40.green/blue}] {pos}/{len} files"
                };
                self.bar.set_style(
                    ProgressStyle::with_template(template)
                        .expect("valid add progress template")
                        .progress_chars("##-"),
                );
            }
            self.bar.set_message(message.to_owned());
        }
        self.bar.set_length(length);
        self.bar.set_position(position);
    }

    fn begin_stage(&mut self, next_stage: AddStage) {
        if self.stage.is_some() {
            self.bar.finish_and_clear();
        }
        self.stage = Some(next_stage);
    }

    fn finish(&self, success: bool) {
        if success {
            self.bar.finish_with_message("Save added");
        } else {
            self.bar.finish_and_clear();
        }
    }
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
            let mut progress = AddProgressReporter::new();
            let result = add_directory_atomically_with_progress(
                &card,
                &save.directory,
                save.directory_mode,
                save.created,
                save.modified,
                &files,
                |event| progress.update(event),
            );
            progress.finish(result.is_ok());
            result?;
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
