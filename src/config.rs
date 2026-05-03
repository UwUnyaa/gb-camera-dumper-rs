use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 1_000;
const DEFAULT_OUTPUT_PATH: &str = "gb-camera.sav";

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
        /// Serial port to try first. If omitted, all serial ports are scanned.
        #[arg(long)]
        port: Option<String>,

        /// Output file path for the raw SRAM dump.
        #[arg(long, default_value = DEFAULT_OUTPUT_PATH)]
        output: PathBuf,

        /// Read timeout for serial operations, in milliseconds.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_MS)]
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone)]
pub struct DumpSramOptions {
    pub port: Option<String>,
    pub output: PathBuf,
    pub timeout_ms: u64,
}

impl DumpSramOptions {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

pub fn parse() -> Command {
    match Cli::parse().command {
        Command::Ports => Command::Ports,
        Command::DumpSram {
            port,
            output,
            timeout_ms,
        } => Command::DumpSram {
            port,
            output,
            timeout_ms,
        },
    }
}
