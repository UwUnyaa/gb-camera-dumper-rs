mod camera;
mod command;
mod constants;
mod config;
mod filename;
mod gbxcart;
mod log;
mod photo;

use anyhow::{Context, Result, bail, ensure};
use chrono::Local;
use command::{Command, DumpSramOptions};
use constants::camera as camera_constants;
use constants::config::{DEFAULT_PHOTO_OUTPUT_DIR, DEFAULT_OUTPUT_PATH, DEFAULT_FILENAME_TEMPLATE};
use gbxcart::{CartridgeMode, GbxcartDevice};
use log::{progress_log, set_debug_logging};
use serialport::SerialPortType;
use std::{fs, path::Path, path::PathBuf};

const PHOTO_EXPORT_SCALE: usize = 2;

fn main() -> Result<()> {
    let (command, config_path) = command::parse();

    // Load config early so CLI options can be merged with config values.
    let cfg = match config::load_config_from_path(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read config: {}. Using defaults.", e);
            config::Config::default()
        }
    };

    match command {
        Command::Ports => list_ports(),
        Command::DumpSram { output, filename_template, debug } => {
            // Resolve final values: CLI > config > defaults
            let final_output: PathBuf = output
                .or(cfg.output_path.clone().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_PATH));

            let final_filename_template: String = filename_template
                .or(cfg.filename_template.clone())
                .unwrap_or_else(|| DEFAULT_FILENAME_TEMPLATE.to_string());

            let options = DumpSramOptions {
                output: final_output,
                filename_template: final_filename_template,
                debug,
            };

            dump_sram(options, cfg)
        }
    }
}


fn list_ports() -> Result<()> {
    let ports = serialport::available_ports().context("failed to enumerate serial ports")?;

    if ports.is_empty() {
        println!("No serial ports detected.");
        return Ok(());
    }

    for port in ports {
        println!("{}", describe_port(&port));
    }

    Ok(())
}

fn dump_sram(options: DumpSramOptions, cfg: config::Config) -> Result<()> {
    set_debug_logging(options.debug);

    progress_log("Detecting GBxCart RW...");
    let (mut device, attempts) = GbxcartDevice::autodetect()?;

    if !attempts.is_empty() {
        eprintln!("Probe attempts before success:");
        for attempt in attempts {
            eprintln!("  {attempt}");
        }
    }

    println!(
        "Connected to {} at {} baud (PCB {}, firmware {}).",
        device.port_name(),
        device.info().baud_rate,
        device.info().pcb_version,
        device.info().firmware_version
    );

    progress_log("Preparing cartridge access...");
    device.prepare_for_game_boy_camera()?;
    ensure!(
        device.info().cartridge_mode == CartridgeMode::GameBoy,
        "the attached GBxCart RW did not enter Game Boy mode"
    );
    let mut header = device.read_cartridge_header()?;
    if header_looks_uninitialized(&header) {
        progress_log("Header looked uninitialized; retrying cartridge prep...");
        device.prepare_for_game_boy_camera()?;
        header = device.read_cartridge_header()?;
    }

    println!(
        "Detected title {:?}, cartridge type 0x{:02X}, ROM size code 0x{:02X}, RAM size code 0x{:02X}.",
        header.title, header.cartridge_type, header.rom_size_code, header.ram_size_code
    );
    ensure!(
        header.logo_ok,
        "the cartridge header logo check failed; re-seat the cartridge and try again"
    );
    ensure!(
        camera::is_game_boy_camera_title(&header.title),
        "the inserted cartridge title {:?} is not a supported Game Boy Camera title",
        header.title
    );
    if header.cartridge_type != 0xFC {
        bail!(
            "the inserted cartridge title matched, but the cartridge type was 0x{:02X} instead of 0xFC",
            header.cartridge_type
        );
    }

    progress_log(&format!(
        "Dumping {} SRAM banks to {}...",
        camera_constants::SRAM_BANK_COUNT,
        options.output.display()
    ));
    device
        .dump_sram(
            &options.output,
            camera_constants::SRAM_BANK_COUNT,
            camera_constants::SRAM_BANK_SIZE,
        )
        .with_context(|| format!("failed to dump SRAM to {}", options.output.display()))?;

    println!(
        "Wrote {} bytes of SRAM to {}.",
        camera_constants::SRAM_SIZE,
        options.output.display()
    );

    // Determine photo output directory from config or derive from output path.
    let photo_output_dir = cfg
        .photo_output_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| photo_output_dir(&options.output));

    progress_log(&format!(
        "Exporting photos to {}...",
        photo_output_dir.display()
    ));
    let mut sram = fs::read(&options.output).with_context(|| {
        format!(
            "failed to read dumped SRAM from {}",
            options.output.display()
        )
    })?;
    let export_time = Local::now().naive_local();

    // Parse palette from config if present: hex colors -> grayscale u8
    let parsed_palette: Option<[u8; 4]> = cfg.palette.as_ref().and_then(|arr| {
        let mut out = [0u8; 4];
        for (i, s) in arr.iter().enumerate() {
            // Accept formats like "#RRGGBB" or "RRGGBB"
            let hex = s.trim_start_matches('#');
            if hex.len() != 6 {
                return None;
            }
            if let Ok(rgb) = u32::from_str_radix(hex, 16) {
                let r = ((rgb >> 16) & 0xFF) as f32;
                let g = ((rgb >> 8) & 0xFF) as f32;
                let b = (rgb & 0xFF) as f32;
                // convert to perceived luminance
                let lum = (0.299 * r + 0.587 * g + 0.114 * b).round() as u8;
                out[i] = lum;
            } else {
                return None;
            }
        }
        Some(out)
    });

    let scale = cfg
        .image_scale
        .unwrap_or(PHOTO_EXPORT_SCALE as u32) as usize;
    let dump_all = cfg.dump_all_photos.unwrap_or(false);

    let photo_count = photo::dump_photos_as_pngs(
        &sram,
        &photo_output_dir,
        scale,
        &options.filename_template,
        export_time,
        dump_all,
        parsed_palette,
    )
    .with_context(|| format!("failed to export photos to {}", photo_output_dir.display()))?;

    println!(
        "Exported {photo_count} photos to {}.",
        photo_output_dir.display()
    );

    if cfg.mark_deleted_after_dump.unwrap_or(false) {
        progress_log("Marking all photos deleted in SRAM as requested by config...");
        photo::mark_all_photos_deleted_in_bytes(&mut sram)?;
        // Overwrite the dumped SRAM file with the modified bytes and write back to cartridge.
        fs::write(&options.output, &sram).with_context(|| {
            format!("failed to write modified SRAM to {}", options.output.display())
        })?;
        device
            .write_sram(
                &options.output,
                camera_constants::SRAM_BANK_COUNT,
                camera_constants::SRAM_BANK_SIZE,
            )
            .with_context(|| format!("failed to write modified SRAM back to cartridge"))?;
        progress_log("Marked photos deleted on cartridge.");
    }

    Ok(())
}

fn photo_output_dir(output_path: &Path) -> std::path::PathBuf {
    output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .join(DEFAULT_PHOTO_OUTPUT_DIR)
}

fn header_looks_uninitialized(header: &gbxcart::CartridgeHeader) -> bool {
    header.title.is_empty()
        && matches!(header.cartridge_type, 0x00 | 0xFF)
        && header.rom_size_code == header.cartridge_type
        && header.ram_size_code == header.cartridge_type
}

fn describe_port(port: &serialport::SerialPortInfo) -> String {
    match &port.port_type {
        SerialPortType::UsbPort(info) => format!(
            "{} - USB VID:{:04x} PID:{:04x} manufacturer={:?} product={:?} serial={:?}",
            port.port_name, info.vid, info.pid, info.manufacturer, info.product, info.serial_number
        ),
        SerialPortType::BluetoothPort => format!("{} - Bluetooth serial port", port.port_name),
        SerialPortType::PciPort => format!("{} - PCI serial port", port.port_name),
        SerialPortType::Unknown => format!("{} - serial port", port.port_name),
    }
}
