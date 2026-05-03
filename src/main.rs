mod camera;
mod config;

use anyhow::{Context, Result};
use config::{Command, Config};
use serialport::{ClearBuffer, SerialPortType};
use std::io::{ErrorKind, Read, Write};
use std::thread;
use std::time::Duration;

fn main() -> Result<()> {
    let (command, config) = config::parse()?;

    match command {
        Command::Ports => list_ports(),
        Command::Probe { request } => probe_device(&config, &request),
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

fn probe_device(config: &Config, request: &str) -> Result<()> {
    let port_name = config
        .port
        .as_deref()
        .context("no serial port configured; set port in gb-camera-dumper.toml")?;

    let mut port = serialport::new(port_name, config.baud_rate)
        .timeout(config.timeout())
        .open()
        .with_context(|| format!("failed to open serial port {port_name}"))?;

    port.clear(ClearBuffer::All)
        .with_context(|| format!("failed to clear buffers on {port_name}"))?;
    port.write_all(request.as_bytes())
        .with_context(|| format!("failed to write probe request to {port_name}"))?;
    port.flush()
        .with_context(|| format!("failed to flush probe request to {port_name}"))?;

    thread::sleep(Duration::from_millis(50));

    let mut response = Vec::new();
    let mut buffer = [0_u8; 256];

    loop {
        match port.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => response.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == ErrorKind::TimedOut => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed while reading from {port_name}"));
            }
        }
    }

    println!("Opened {port_name} at {} baud.", config.baud_rate);
    if response.is_empty() {
        println!(
            "Sent {:?}, but no bytes were returned before timeout.",
            request
        );
    } else {
        println!("Response (hex): {}", format_hex(&response));
        println!("Response (text): {}", String::from_utf8_lossy(&response));
    }

    Ok(())
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

fn format_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
