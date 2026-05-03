use crate::constants::config::DEFAULT_OUTPUT_PATH;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Proof-of-concept GBxCart RW Game Boy Camera SRAM dumper"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List serial ports and USB metadata.
    Ports,
    /// Auto-detect a GBxCart RW, confirm a supported Game Boy Camera cartridge, and dump SRAM.
    DumpSram {
        /// Output file path for the raw SRAM dump.
        #[arg(long, default_value = DEFAULT_OUTPUT_PATH)]
        output: PathBuf,

        /// Enable serial debug logging.
        #[arg(long)]
        debug: bool,
    },
}

#[derive(Debug, Clone)]
pub struct DumpSramOptions {
    pub output: PathBuf,
    pub debug: bool,
}

pub fn parse() -> Command {
    match Cli::parse().command {
        Command::Ports => Command::Ports,
        Command::DumpSram { output, debug } => Command::DumpSram { output, debug },
    }
}
