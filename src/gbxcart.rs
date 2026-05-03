use anyhow::{Context, Result, anyhow, bail};
use serialport::{ClearBuffer, SerialPort};
use std::env;
use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const PROBE_TIMEOUT_MS: u64 = 1_000;
const BAUD_RATES: [u32; 3] = [1_000_000, 1_700_000, 1_500_000];
const STREAM_BLOCK_SIZE: usize = 64;
const HEADER_READ_LENGTH: usize = 0x180;
const CART_MODE_COMMAND: u8 = b'C';
const READ_PCB_VERSION_COMMAND: u8 = b'h';
const READ_FIRMWARE_VERSION_COMMAND: u8 = b'V';
const GB_CART_MODE_COMMAND: u8 = b'G';
const STOP_STREAM_COMMAND: u8 = b'0';
const VOLTAGE_5V_COMMAND: u8 = b'5';
const QUERY_CART_POWER_COMMAND: u8 = b']';
const POWER_CART_ON_COMMAND: u8 = b'/';
const RESET_AVR_COMMAND: u8 = b'*';
const CART_POWER_ON_BINARY_COMMAND: u8 = 0xF2;
const QUERY_CART_POWER_BINARY_COMMAND: u8 = 0xF4;
const SET_MODE_DMG_COMMAND: u8 = 0xA3;
const SET_VOLTAGE_5V_BINARY_COMMAND: u8 = 0xA5;
const SET_VARIABLE_COMMAND: u8 = 0xA6;
const DISABLE_PULLUPS_COMMAND: u8 = 0xAC;
const DMG_CART_READ_COMMAND: u8 = 0xB1;
const DMG_CART_WRITE_COMMAND: u8 = 0xB2;
const DMG_MBC_RESET_COMMAND: u8 = 0xB4;

const CART_MODE_GB: u8 = 1;
const CART_MODE_GBA: u8 = 2;
const PCB_1_3: u8 = 4;
const PCB_1_4: u8 = 5;
const PCB_GBXMAS: u8 = 90;
const FW_VAR_ADDRESS: u32 = 0x00;
const FW_VAR_TRANSFER_SIZE: u32 = 0x00;
const FW_VAR_CART_MODE: u32 = 0x00;
const FW_VAR_DMG_ACCESS_MODE: u32 = 0x01;
const FW_VAR_DMG_READ_CS_PULSE: u32 = 0x08;
const DMG_ACCESS_MODE_ROM_READ: u32 = 0x01;
const DMG_ACCESS_MODE_RAM_READ: u32 = 0x03;

const NINTENDO_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static DEBUG_VERBOSE_ENABLED: AtomicBool = AtomicBool::new(false);

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
    pub fn autodetect() -> Result<(Self, Vec<String>)> {
        let timeout = Duration::from_millis(PROBE_TIMEOUT_MS);
        let ports = serialport::available_ports()
            .context("failed to enumerate serial ports for probing")?;
        if ports.is_empty() {
            bail!("no serial ports detected");
        }

        let ordered_ports = ordered_port_names(&ports);
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
        if self.info.firmware_version >= 12 {
            debug_log("using modern firmware prep path");
            let power_query = self.request_value(QUERY_CART_POWER_BINARY_COMMAND);
            match &power_query {
                Ok(value) => debug_log(&format!("binary query cart power returned 0x{value:02X}")),
                Err(error) => debug_log(&format!("binary query cart power failed: {error:#}")),
            }

            self.send_command_expect_ack(SET_MODE_DMG_COMMAND, "set DMG mode")?;
            self.send_command_expect_ack(SET_VOLTAGE_5V_BINARY_COMMAND, "set 5V")?;
            self.send_command_expect_ack(DISABLE_PULLUPS_COMMAND, "disable pullups")?;
            self.set_fw_variable(1, FW_VAR_CART_MODE, 1)?;
            self.set_fw_variable(4, FW_VAR_ADDRESS, 0)?;

            if matches!(power_query, Ok(0)) {
                self.send_command_expect_ack(CART_POWER_ON_BINARY_COMMAND, "power on cartridge")?;
                thread::sleep(Duration::from_millis(200));
            }

            self.send_command_expect_ack(DMG_MBC_RESET_COMMAND, "reset DMG mapper")?;
            thread::sleep(Duration::from_millis(150));
            self.clear_buffers()?;
            self.info.cartridge_mode = CartridgeMode::GameBoy;
            debug_log("modern firmware prep completed");
            return Ok(());
        }

        let power_query = self.request_value(QUERY_CART_POWER_COMMAND);
        match &power_query {
            Ok(value) => debug_log(&format!("query cart power returned 0x{value:02X}")),
            Err(error) => debug_log(&format!("query cart power failed: {error:#}")),
        }
        let should_power_on = match power_query {
            Ok(0) => true,
            Ok(_) => false,
            Err(_) => self.info.firmware_version != 0,
        };

        if matches!(self.info.pcb_version, PCB_1_3 | PCB_1_4 | PCB_GBXMAS)
            || self.info.firmware_version != 0
        {
            debug_log("sending legacy 5V command");
            self.send_command(VOLTAGE_5V_COMMAND)
                .context("failed to switch the GBxCart RW to 5V mode")?;
            thread::sleep(Duration::from_millis(500));
            self.clear_buffers().ok();
        }

        if should_power_on {
            debug_log("sending legacy cart power on command");
            self.send_command(POWER_CART_ON_COMMAND)
                .context("failed to power on the cartridge")?;
            thread::sleep(Duration::from_millis(500));
            self.clear_buffers().ok();
        }

        debug_log("switching to DMG mode and resetting mapper");
        self.send_command(GB_CART_MODE_COMMAND)
            .context("failed to switch the GBxCart RW into Game Boy cart mode")?;
        self.send_command(SET_MODE_DMG_COMMAND)
            .context("failed to switch the GBxCart RW into binary Game Boy mode")?;
        self.send_command(SET_VOLTAGE_5V_BINARY_COMMAND)
            .context("failed to switch the GBxCart RW into binary 5V mode")?;
        self.send_command(DMG_MBC_RESET_COMMAND)
            .context("failed to reset the DMG mapper before reading")?;
        thread::sleep(Duration::from_millis(150));
        self.clear_buffers()?;
        self.info.cartridge_mode = self
            .request_value(CART_MODE_COMMAND)
            .context("failed to confirm Game Boy cart mode after switching")?
            .try_into()
            .map_err(|mode: u8| anyhow!("unexpected cart mode response 0x{mode:02X}"))?;
        debug_log(&format!("confirmed cart mode after prep: {:?}", self.info.cartridge_mode));

        Ok(())
    }

    pub fn read_cartridge_header(&mut self) -> Result<CartridgeHeader> {
        let header = self
            .read_dmg_rom(0, HEADER_READ_LENGTH)
            .context("failed to read the cartridge header")?;
        debug_log(&format!(
            "header bytes: {}",
            format_bytes(&header[..header.len().min(64)])
        ));
        let header: [u8; HEADER_READ_LENGTH] = header
            .try_into()
            .map_err(|_| anyhow!("header read returned an unexpected number of bytes"))?;
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
                debug_log(&format!("dumping SRAM bank {bank} of {}", bank_count - 1));
                self.set_bank(0x4000, bank)
                    .with_context(|| format!("failed to select SRAM bank {bank}"))?;
                for block_offset in (0..bank_size).step_by(STREAM_BLOCK_SIZE) {
                    let block = self
                        .read_dmg_ram(block_offset as u16, STREAM_BLOCK_SIZE)
                        .with_context(|| {
                            format!(
                                "failed to read block {} from SRAM bank {bank}",
                                block_offset / STREAM_BLOCK_SIZE
                            )
                        })?;
                    file.write_all(&block).with_context(|| {
                        format!(
                            "failed to write block {} for SRAM bank {bank}",
                            block_offset / STREAM_BLOCK_SIZE
                        )
                    })?;
                }
            }

            Ok(())
        })();

        let disable_result = self.set_bank(0x0000, 0x00);
        write_result?;
        disable_result.context("failed to disable cartridge RAM after dumping SRAM")?;
        Ok(())
    }

    fn try_connect(port_name: &str, baud_rate: u32, timeout: Duration) -> Result<Self> {
        debug_log(&format!("opening {port_name} at {baud_rate} baud"));
        let mut port = serialport::new(port_name, baud_rate)
            .timeout(timeout)
            .open()
            .with_context(|| format!("failed to open {port_name}"))?;

        clear_buffers(&mut *port)
            .with_context(|| format!("failed to clear serial buffers on {port_name}"))?;
        debug_log(&format!("sending stop stream to {port_name} at {baud_rate}"));
        send_command(&mut *port, STOP_STREAM_COMMAND)
            .with_context(|| format!("failed to reset the device state on {port_name}"))?;
        clear_buffers(&mut *port).ok();

        debug_log(&format!("sending reset avr to {port_name} at {baud_rate}"));
        let _ = send_command(&mut *port, RESET_AVR_COMMAND);
        thread::sleep(Duration::from_millis(500));
        clear_buffers(&mut *port).ok();

        let cart_mode = request_value(&mut *port, CART_MODE_COMMAND)
            .with_context(|| format!("failed to probe {port_name}"))?;
        let cartridge_mode = CartridgeMode::from_byte(cart_mode)
            .ok_or_else(|| anyhow!("unexpected cart mode response 0x{cart_mode:02X}"))?;
        let pcb_version = request_value(&mut *port, READ_PCB_VERSION_COMMAND)
            .with_context(|| format!("failed to read PCB version from {port_name}"))?;
        let firmware_version = request_value(&mut *port, READ_FIRMWARE_VERSION_COMMAND)
            .with_context(|| format!("failed to read firmware version from {port_name}"))?;

        debug_log(&format!(
            "probe {port_name} @ {baud_rate}: cart_mode=0x{cart_mode:02X} pcb=0x{pcb_version:02X} fw=0x{firmware_version:02X}"
        ));

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

    fn request_value(&mut self, command: u8) -> Result<u8> {
        request_value(&mut *self.port, command)
    }

    fn set_bank(&mut self, address: u16, bank: u8) -> Result<()> {
        self.write_dmg_cart(address, bank)?;
        thread::sleep(Duration::from_millis(5));
        Ok(())
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

    fn set_fw_variable(&mut self, size: u8, key: u32, value: u32) -> Result<()> {
        let mut buffer = Vec::with_capacity(10);
        buffer.push(SET_VARIABLE_COMMAND);
        buffer.push(size);
        buffer.extend_from_slice(&key.to_be_bytes());
        buffer.extend_from_slice(&value.to_be_bytes());
        debug_log(&format!(
            "set variable size={size} key=0x{key:08X} value=0x{value:08X}"
        ));
        self.port
            .write_all(&buffer)
            .context("failed to write firmware variable command")?;
        self.port
            .flush()
            .context("failed to flush firmware variable command")?;
        self.read_ack("set firmware variable")?;
        Ok(())
    }

    fn write_dmg_cart(&mut self, address: u16, value: u8) -> Result<()> {
        let mut buffer = Vec::with_capacity(6);
        buffer.push(DMG_CART_WRITE_COMMAND);
        buffer.extend_from_slice(&u32::from(address).to_be_bytes());
        buffer.push(value);
        debug_log(&format!("write dmg cart address=0x{address:04X} value=0x{value:02X}"));
        self.port
            .write_all(&buffer)
            .context("failed to write DMG cart write command")?;
        self.port
            .flush()
            .context("failed to flush DMG cart write command")?;
        self.read_ack("DMG cart write")?;
        Ok(())
    }

    fn read_dmg_rom(&mut self, address: u32, length: usize) -> Result<Vec<u8>> {
        debug_log(&format!("read dmg rom address=0x{address:08X} length=0x{length:X}"));
        self.set_fw_variable(2, FW_VAR_TRANSFER_SIZE, length as u32)?;
        self.set_fw_variable(4, FW_VAR_ADDRESS, address)?;
        self.set_fw_variable(1, FW_VAR_DMG_ACCESS_MODE, DMG_ACCESS_MODE_ROM_READ)?;
        self.send_command(DMG_CART_READ_COMMAND)
            .context("failed to start DMG ROM read")?;

        let mut buffer = vec![0_u8; length];
        self.read_exact_into(&mut buffer)
            .context("failed to read DMG ROM bytes")?;
        Ok(buffer)
    }

    fn read_dmg_ram(&mut self, address: u16, length: usize) -> Result<Vec<u8>> {
        debug_log_verbose(&format!("read dmg ram address=0x{address:04X} length=0x{length:X}"));
        self.set_fw_variable(2, FW_VAR_TRANSFER_SIZE, length as u32)?;
        self.set_fw_variable(4, FW_VAR_ADDRESS, u32::from(0xA000_u16 + address))?;
        self.set_fw_variable(1, FW_VAR_DMG_ACCESS_MODE, DMG_ACCESS_MODE_RAM_READ)?;
        self.set_fw_variable(1, FW_VAR_DMG_READ_CS_PULSE, 1)?;
        self.send_command(DMG_CART_READ_COMMAND)
            .context("failed to start DMG RAM read")?;

        let read_result = (|| -> Result<Vec<u8>> {
            let mut buffer = vec![0_u8; length];
            self.read_exact_into(&mut buffer)
                .context("failed to read DMG RAM bytes")?;
            Ok(buffer)
        })();

        self.set_fw_variable(1, FW_VAR_DMG_READ_CS_PULSE, 0)
            .context("failed to restore DMG read CS pulse setting")?;
        read_result
    }

    fn read_ack(&mut self, context: &str) -> Result<()> {
        if self.info.firmware_version < 12 {
            return Ok(());
        }

        let mut ack = [0_u8; 1];
        self.read_exact_into(&mut ack)
            .with_context(|| format!("failed to read ACK for {context}"))?;
        debug_log(&format!("ack for {context}: 0x{:02X}", ack[0]));
        match ack[0] {
            0x01 | 0x03 => Ok(()),
            other => bail!("unexpected ACK 0x{other:02X} for {context}"),
        }
    }

    fn send_command_expect_ack(&mut self, command: u8, context: &str) -> Result<()> {
        self.send_command(command)?;
        self.read_ack(context)
    }
}

pub fn set_debug_logging(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn set_verbose_debug_logging(enabled: bool) {
    DEBUG_VERBOSE_ENABLED.store(enabled, Ordering::Relaxed);
}

fn ordered_port_names(ports: &[serialport::SerialPortInfo]) -> Vec<String> {
    let mut names = Vec::with_capacity(ports.len());

    for port in ports {
        if names.iter().all(|name| name != &port.port_name) {
            names.push(port.port_name.clone());
        }
    }

    names
}

fn clear_buffers(port: &mut dyn SerialPort) -> Result<()> {
    debug_log("clearing serial buffers");
    port.clear(ClearBuffer::All)
        .context("serial clear operation failed")
}

fn send_command(port: &mut dyn SerialPort, command: u8) -> Result<()> {
    debug_log(&format!("send command 0x{command:02X}"));
    port.write_all(&[command])
        .context("failed to write serial command")?;
    port.flush().context("failed to flush serial command")
}

fn request_value(port: &mut dyn SerialPort, command: u8) -> Result<u8> {
    send_command(port, command)?;
    let mut value = [0_u8; 1];
    loop {
        match port.read(&mut value) {
            Ok(0) => continue,
            Ok(_) => {
                debug_log(&format!("response to 0x{command:02X}: 0x{:02X}", value[0]));
                return Ok(value[0]);
            }
            Err(error) if error.kind() == ErrorKind::TimedOut => {
                return Err(error).context("timed out while waiting for a probe response");
            }
            Err(error) => return Err(error).context("failed while reading a probe response"),
        }
    }
}

fn debug_log(message: &str) {
    if DEBUG_ENABLED.load(Ordering::Relaxed) || env::var_os("GB_CAMERA_DEBUG").is_some() {
        eprintln!("[gb-camera-debug] {message}");
    }
}

fn debug_log_verbose(message: &str) {
    if DEBUG_VERBOSE_ENABLED.load(Ordering::Relaxed)
        || env::var_os("GB_CAMERA_DEBUG_VERBOSE").is_some()
    {
        eprintln!("[gb-camera-debug] {message}");
    }
}

fn format_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
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
