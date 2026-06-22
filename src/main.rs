mod camera;
mod command;
mod constants;
mod filename;
mod gbxcart;
mod log;
mod photo;

use anyhow::{Context, Result, bail, ensure};
use chrono::Local;
use command::{Command, DumpSramOptions};
use constants::camera as camera_constants;
use constants::config::DEFAULT_PHOTO_OUTPUT_DIR;
use gbxcart::{CartridgeMode, GbxcartDevice};
use log::{progress_log, set_debug_logging};
use serialport::SerialPortType;
use std::{fs, path::Path};

const PHOTO_EXPORT_SCALE: usize = 2;

fn main() -> Result<()> {
    let command = command::parse();

    match command {
        Command::Ports => list_ports(),
        Command::DumpSram {
            output,
            filename_template,
            debug,
        } => dump_sram(DumpSramOptions {
            output,
            filename_template,
            debug,
        }),
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

fn dump_sram(options: DumpSramOptions) -> Result<()> {
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

    let photo_output_dir = photo_output_dir(&options.output);
    progress_log(&format!(
        "Exporting photos to {}...",
        photo_output_dir.display()
    ));
    let sram = fs::read(&options.output).with_context(|| {
        format!(
            "failed to read dumped SRAM from {}",
            options.output.display()
        )
    })?;
    let export_time = Local::now().naive_local();
    let photo_count = photo::dump_active_photos_as_pngs(
        &sram,
        &photo_output_dir,
        PHOTO_EXPORT_SCALE,
        &options.filename_template,
        export_time,
    )
    .with_context(|| format!("failed to export photos to {}", photo_output_dir.display()))?;
    println!(
        "Exported {photo_count} undeleted photos to {}.",
        photo_output_dir.display()
    );

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
