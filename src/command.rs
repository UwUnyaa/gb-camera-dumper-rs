use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Proof-of-concept GBxCart RW Game Boy Camera SRAM dumper"
)]
struct Cli {
    /// Path to a YAML config file. If provided, this overrides the usual
    /// $XDG_CONFIG_HOME/... or $HOME/.gb-camera-dumper-config.yaml locations.
    #[arg(short = 'c', long = "config")]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
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

pub fn parse() -> (Command, Option<std::path::PathBuf>) {
    let cli = Cli::parse();
    let cmd = match cli.command {
        Some(Command::Ports) => Command::Ports,
        Some(Command::DumpSram { output, filename_template, debug }) => Command::DumpSram {
            output,
            filename_template,
            debug,
        },
        None => {
            // No subcommand provided: default to DumpSram with no overrides so
            // the program will read the config file (if present) and use its
            // settings.
            Command::DumpSram {
                output: None,
                filename_template: None,
                debug: false,
            }
        }
    };
    (cmd, cli.config)
}
