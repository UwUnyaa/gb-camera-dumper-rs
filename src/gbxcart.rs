use anyhow::{Context, Result, anyhow, bail};
use serialport::{ClearBuffer, SerialPort};
use std::io::{ErrorKind, Read, Write};
use std::thread;
use std::time::Duration;

const BAUD_RATES: [u32; 2] = [1_000_000, 1_700_000];
const STREAM_BLOCK_SIZE: usize = 64;
const HEADER_READ_LENGTH: usize = 0x180;
const CART_MODE_COMMAND: u8 = b'C';
const READ_PCB_VERSION_COMMAND: u8 = b'h';
const READ_FIRMWARE_VERSION_COMMAND: u8 = b'V';
const SET_START_ADDRESS_COMMAND: u8 = b'A';
const READ_ROM_RAM_COMMAND: u8 = b'R';
const GB_CART_MODE_COMMAND: u8 = b'G';
const SET_BANK_COMMAND: u8 = b'B';
const STOP_STREAM_COMMAND: u8 = b'0';
const CONTINUE_STREAM_COMMAND: u8 = b'1';
const VOLTAGE_5V_COMMAND: u8 = b'5';
const QUERY_CART_POWER_COMMAND: u8 = b']';
const POWER_CART_ON_COMMAND: u8 = b'/';

const CART_MODE_GB: u8 = 1;
const CART_MODE_GBA: u8 = 2;
const PCB_1_3: u8 = 4;
const PCB_1_4: u8 = 5;
const PCB_GBXMAS: u8 = 90;

const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CartridgeMode {
    GameBoy,
    GameBoyAdvance,
}

impl CartridgeMode {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            CART_MODE_GB => Some(Self::GameBoy),
            CART_MODE_GBA => Some(Self::GameBoyAdvance),
            _ => None,
        }
    }
}

impl TryFrom<u8> for CartridgeMode {
    type Error = u8;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        Self::from_byte(value).ok_or(value)
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub baud_rate: u32,
    pub cartridge_mode: CartridgeMode,
    pub pcb_version: u8,
    pub firmware_version: u8,
}

#[derive(Debug, Clone)]
pub struct CartridgeHeader {
    pub title: String,
    pub cartridge_type: u8,
    pub rom_size_code: u8,
    pub ram_size_code: u8,
    pub logo_ok: bool,
}

pub struct GbxcartDevice {
    port_name: String,
    port: Box<dyn SerialPort>,
    info: DeviceInfo,
}

impl GbxcartDevice {
    pub fn autodetect(
        preferred_port: Option<&str>,
        timeout: Duration,
    ) -> Result<(Self, Vec<String>)> {
        let ports = serialport::available_ports()
            .context("failed to enumerate serial ports for probing")?;
        if ports.is_empty() {
            bail!("no serial ports detected");
        }

        let ordered_ports = ordered_port_names(&ports, preferred_port);
        let mut attempts = Vec::new();

        for port_name in ordered_ports {
            for baud_rate in BAUD_RATES {
                match Self::try_connect(&port_name, baud_rate, timeout) {
                    Ok(device) => return Ok((device, attempts)),
                    Err(error) => attempts.push(format!("{port_name} @ {baud_rate}: {error}")),
                }
            }
        }

        let mut message = String::from("could not find a GBxCart RW on the available serial ports");
        if !attempts.is_empty() {
            message.push_str(":\n");
            message.push_str(&attempts.join("\n"));
        }
        Err(anyhow!(message))
    }

    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn prepare_for_game_boy_camera(&mut self) -> Result<()> {
        if matches!(self.info.pcb_version, PCB_1_3 | PCB_1_4 | PCB_GBXMAS) {
            self.send_command(VOLTAGE_5V_COMMAND)
                .context("failed to switch the GBxCart RW to 5V mode")?;
            thread::sleep(Duration::from_millis(500));
        }

        if self.info.pcb_version == PCB_1_4 {
            let cart_powered = self
                .request_value(QUERY_CART_POWER_COMMAND)
                .context("failed to query cartridge power state")?;
            if cart_powered == 0 {
                self.send_command(POWER_CART_ON_COMMAND)
                    .context("failed to power on the cartridge")?;
                thread::sleep(Duration::from_millis(500));
                self.clear_buffers()?;
            }
        }

        self.send_command(GB_CART_MODE_COMMAND)
            .context("failed to switch the GBxCart RW into Game Boy cart mode")?;
        thread::sleep(Duration::from_millis(10));
        self.clear_buffers()?;
        self.info.cartridge_mode = self
            .request_value(CART_MODE_COMMAND)
            .context("failed to confirm Game Boy cart mode after switching")?
            .try_into()
            .map_err(|mode: u8| anyhow!("unexpected cart mode response 0x{mode:02X}"))?;

        Ok(())
    }

    pub fn read_cartridge_header(&mut self) -> Result<CartridgeHeader> {
        self.send_number(SET_START_ADDRESS_COMMAND, 0)
            .context("failed to set the header read start address")?;
        self.send_command(READ_ROM_RAM_COMMAND)
            .context("failed to start ROM streaming for header read")?;

        let read_result = (|| -> Result<[u8; HEADER_READ_LENGTH]> {
            let mut header = [0_u8; HEADER_READ_LENGTH];
            for (index, chunk) in header.chunks_mut(STREAM_BLOCK_SIZE).enumerate() {
                self.read_exact_into(chunk)
                    .with_context(|| format!("failed to read header chunk {index}"))?;
                if index + 1 != HEADER_READ_LENGTH / STREAM_BLOCK_SIZE {
                    self.send_command(CONTINUE_STREAM_COMMAND)
                        .context("failed to request the next header chunk")?;
                }
            }
            Ok(header)
        })();

        self.stop_stream()
            .context("failed to stop ROM streaming after header read")?;

        let header = read_result?;
        Ok(parse_cartridge_header(&header))
    }

    pub fn dump_sram(
        &mut self,
        output: &std::path::Path,
        bank_count: usize,
        bank_size: usize,
    ) -> Result<()> {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create output directory {}", parent.display())
                })?;
            }
        }

        let write_result = (|| -> Result<()> {
            let mut file = std::fs::File::create(output)
                .with_context(|| format!("failed to create {}", output.display()))?;

            self.set_bank(0x0000, 0x0A)
                .context("failed to enable cartridge RAM")?;

            for bank in 0..bank_count {
                let bank = u8::try_from(bank).context("SRAM bank index overflowed u8")?;
                self.set_bank(0x4000, bank)
                    .with_context(|| format!("failed to select SRAM bank {bank}"))?;
                self.send_number(SET_START_ADDRESS_COMMAND, 0xA000)
                    .with_context(|| format!("failed to set SRAM read start for bank {bank}"))?;
                self.send_command(READ_ROM_RAM_COMMAND)
                    .with_context(|| format!("failed to start SRAM streaming for bank {bank}"))?;

                let bank_read_result = (|| -> Result<()> {
                    let block_count = bank_size / STREAM_BLOCK_SIZE;
                    let mut block = [0_u8; STREAM_BLOCK_SIZE];
                    for block_index in 0..block_count {
                        self.read_exact_into(&mut block).with_context(|| {
                            format!("failed to read block {block_index} from SRAM bank {bank}")
                        })?;
                        file.write_all(&block).with_context(|| {
                            format!("failed to write block {block_index} for SRAM bank {bank}")
                        })?;
                        if block_index + 1 != block_count {
                            self.send_command(CONTINUE_STREAM_COMMAND)
                                .with_context(|| {
                                    format!(
                                        "failed to request block {} from SRAM bank {bank}",
                                        block_index + 1
                                    )
                                })?;
                        }
                    }
                    Ok(())
                })();

                self.stop_stream()
                    .with_context(|| format!("failed to stop SRAM streaming for bank {bank}"))?;
                bank_read_result?;
            }

            Ok(())
        })();

        let disable_result = self.set_bank(0x0000, 0x00);
        write_result?;
        disable_result.context("failed to disable cartridge RAM after dumping SRAM")?;
        Ok(())
    }

    fn try_connect(port_name: &str, baud_rate: u32, timeout: Duration) -> Result<Self> {
        let mut port = serialport::new(port_name, baud_rate)
            .timeout(timeout)
            .open()
            .with_context(|| format!("failed to open {port_name}"))?;

        clear_buffers(&mut *port)
            .with_context(|| format!("failed to clear serial buffers on {port_name}"))?;
        send_command(&mut *port, STOP_STREAM_COMMAND)
            .with_context(|| format!("failed to reset the device state on {port_name}"))?;
        clear_buffers(&mut *port).ok();

        let cart_mode = request_value(&mut *port, CART_MODE_COMMAND)
            .with_context(|| format!("failed to probe {port_name}"))?;
        let cartridge_mode = CartridgeMode::from_byte(cart_mode)
            .ok_or_else(|| anyhow!("unexpected cart mode response 0x{cart_mode:02X}"))?;
        let pcb_version = request_value(&mut *port, READ_PCB_VERSION_COMMAND)
            .with_context(|| format!("failed to read PCB version from {port_name}"))?;
        let firmware_version = request_value(&mut *port, READ_FIRMWARE_VERSION_COMMAND)
            .with_context(|| format!("failed to read firmware version from {port_name}"))?;

        Ok(Self {
            port_name: port_name.to_owned(),
            port,
            info: DeviceInfo {
                baud_rate,
                cartridge_mode,
                pcb_version,
                firmware_version,
            },
        })
    }

    fn clear_buffers(&mut self) -> Result<()> {
        clear_buffers(&mut *self.port)
    }

    fn send_command(&mut self, command: u8) -> Result<()> {
        send_command(&mut *self.port, command)
    }

    fn send_number(&mut self, command: u8, value: u32) -> Result<()> {
        send_number(&mut *self.port, command, value)
    }

    fn request_value(&mut self, command: u8) -> Result<u8> {
        request_value(&mut *self.port, command)
    }

    fn set_bank(&mut self, address: u16, bank: u8) -> Result<()> {
        self.send_number(SET_BANK_COMMAND, u32::from(address))?;
        thread::sleep(Duration::from_millis(5));
        self.send_number(SET_BANK_COMMAND, u32::from(bank))?;
        thread::sleep(Duration::from_millis(5));
        Ok(())
    }

    fn stop_stream(&mut self) -> Result<()> {
        self.send_command(STOP_STREAM_COMMAND)
    }

    fn read_exact_into(&mut self, buffer: &mut [u8]) -> Result<()> {
        let mut offset = 0;
        while offset < buffer.len() {
            match self.port.read(&mut buffer[offset..]) {
                Ok(0) => continue,
                Ok(count) => offset += count,
                Err(error) if error.kind() == ErrorKind::TimedOut => {
                    return Err(error).context("timed out while waiting for serial data");
                }
                Err(error) => return Err(error).context("failed while reading serial data"),
            }
        }
        Ok(())
    }
}

fn ordered_port_names(
    ports: &[serialport::SerialPortInfo],
    preferred_port: Option<&str>,
) -> Vec<String> {
    let mut names = Vec::with_capacity(ports.len());
    if let Some(preferred_port) = preferred_port {
        if ports.iter().any(|port| port.port_name == preferred_port) {
            names.push(preferred_port.to_owned());
        }
    }

    for port in ports {
        if names.iter().all(|name| name != &port.port_name) {
            names.push(port.port_name.clone());
        }
    }

    names
}

fn clear_buffers(port: &mut dyn SerialPort) -> Result<()> {
    port.clear(ClearBuffer::All)
        .context("serial clear operation failed")
}

fn send_command(port: &mut dyn SerialPort, command: u8) -> Result<()> {
    port.write_all(&[command])
        .context("failed to write serial command")?;
    port.flush().context("failed to flush serial command")
}

fn send_number(port: &mut dyn SerialPort, command: u8, value: u32) -> Result<()> {
    let message = format!("{}{value:x}\0", char::from(command));
    port.write_all(message.as_bytes())
        .context("failed to write serial numeric command")?;
    port.flush()
        .context("failed to flush serial numeric command")
}

fn request_value(port: &mut dyn SerialPort, command: u8) -> Result<u8> {
    send_command(port, command)?;
    let mut value = [0_u8; 1];
    loop {
        match port.read(&mut value) {
            Ok(0) => continue,
            Ok(_) => return Ok(value[0]),
            Err(error) if error.kind() == ErrorKind::TimedOut => {
                return Err(error).context("timed out while waiting for a probe response");
            }
            Err(error) => return Err(error).context("failed while reading a probe response"),
        }
    }
}

fn parse_cartridge_header(bytes: &[u8; HEADER_READ_LENGTH]) -> CartridgeHeader {
    let title_bytes = &bytes[0x0134..0x0144];
    let title = title_bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0 && *byte != 0xFF)
        .filter(|byte| byte.is_ascii_graphic() || *byte == b' ')
        .map(char::from)
        .collect::<String>();

    let logo_ok = bytes[0x0104..0x0134] == NINTENDO_LOGO;

    CartridgeHeader {
        title,
        cartridge_type: bytes[0x0147],
        rom_size_code: bytes[0x0148],
        ram_size_code: bytes[0x0149],
        logo_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_camera_header_fields() {
        let mut header = [0_u8; HEADER_READ_LENGTH];
        header[0x0104..0x0134].copy_from_slice(&NINTENDO_LOGO);
        header[0x0134..0x0141].copy_from_slice(b"GAMEBOYCAMERA");
        header[0x0147] = 0xFC;
        header[0x0148] = 0x01;
        header[0x0149] = 0x04;

        let parsed = parse_cartridge_header(&header);

        assert_eq!(parsed.title, "GAMEBOYCAMERA");
        assert_eq!(parsed.cartridge_type, 0xFC);
        assert_eq!(parsed.rom_size_code, 0x01);
        assert_eq!(parsed.ram_size_code, 0x04);
        assert!(parsed.logo_ok);
    }
}
