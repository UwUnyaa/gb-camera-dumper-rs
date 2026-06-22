use crate::constants::config::{DEFAULT_FILENAME_TEMPLATE, DEFAULT_OUTPUT_PATH};
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
        /// When omitted, the config file or default will be used.
        #[arg(long)]
        output: Option<PathBuf>,

        /// PNG filename template.
        ///
        /// Supported fields: {year}, {month}, {day}, {hour24}, {hour12},
        /// {minute}, {sequential}, and {slot}. Add a width like {slot:02}
        /// to zero-pad numbers. When omitted, the config or default is used.
        #[arg(long)]
        filename_template: Option<String>,

        /// Enable serial debug logging.
        #[arg(long)]
        debug: bool,
    },

}

#[derive(Debug, Clone)]
pub struct DumpSramOptions {
    pub output: PathBuf,
    pub filename_template: String,
    pub debug: bool,
}

pub fn parse() -> Command {
    match Cli::parse().command {
        Command::Ports => Command::Ports,
        Command::DumpSram {
            output,
            filename_template,
            debug,
        } => Command::DumpSram {
            output,
            filename_template,
            debug,
        },
    }
}
