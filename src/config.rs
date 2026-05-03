use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_BAUD_RATE: u32 = 1_000_000;
const DEFAULT_TIMEOUT_MS: u64 = 1_000;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "GBxCart RW CLI scaffold with serial transport and TOML configuration"
)]
struct Cli {
    /// Path to a TOML configuration file.
    #[arg(long, default_value = "gb-camera-dumper.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List serial ports that could correspond to a GBxCart RW reader.
    Ports,
    /// Open the configured port and send a small ASCII request.
    Probe {
        /// ASCII request to send after opening the port.
        #[arg(long, default_value = "S")]
        request: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub port: Option<String>,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: None,
            baud_rate: default_baud_rate(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

impl Config {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

pub fn parse() -> Result<(Command, Config)> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;
    Ok((cli.command, config))
}

fn load_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("failed to parse TOML config {}", path.display()))
}

const fn default_baud_rate() -> u32 {
    DEFAULT_BAUD_RATE
}

const fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}
