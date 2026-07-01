use crate::constants::gbxcart::*;
use crate::log::{debug_log, debug_log_verbose, progress_log};
use anyhow::{Context, Result, anyhow, bail};
use serialport::{ClearBuffer, SerialPort};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

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
        progress_log(&format!(
            "Scanning {} serial ports for a GBxCart RW...",
            ordered_ports.len()
        ));
        let mut attempts = Vec::new();

        for port_name in ordered_ports {
            for baud_rate in BAUD_RATES {
                match Self::try_connect(&port_name, baud_rate, timeout) {
                    Ok(device) => {
                        progress_log(&format!(
                            "Detected GBxCart RW on {port_name} at {baud_rate} baud."
                        ));
                        return Ok((device, attempts));
                    }
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
        if self.uses_modern_firmware() {
            return self.prepare_for_game_boy_camera_modern();
        }

        self.prepare_for_game_boy_camera_legacy()
    }

    pub fn read_cartridge_header(&mut self) -> Result<CartridgeHeader> {
        progress_log("Reading the cartridge header...");
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

    pub fn dump_sram(&mut self, output: &Path, bank_count: usize, bank_size: usize) -> Result<()> {
        self.ensure_parent_directory(output)?;

        let write_result = self.dump_sram_to_file(output, bank_count, bank_size);
        let disable_result = self.disable_cartridge_ram();
        write_result?;
        disable_result.context("failed to disable cartridge RAM after dumping SRAM")?;
        Ok(())
    }

    /// Dump the cartridge SRAM directly into an in-memory Vec<u8> instead of
    /// creating a file on disk. The returned Vec will contain bank_count * bank_size
    /// bytes on success. The cartridge RAM will be disabled before returning.
    pub fn dump_sram_to_vec(&mut self, bank_count: usize, bank_size: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(bank_count * bank_size);

        self.enable_cartridge_ram()?;
        for bank in 0..bank_count {
            let bank_u8 = u8::try_from(bank).context("SRAM bank index overflowed u8")?;
            self.select_sram_bank(bank_u8)?;

            for block_offset in (0..bank_size).step_by(STREAM_BLOCK_SIZE) {
                let chunk = self
                    .read_dmg_ram(block_offset as u16, STREAM_BLOCK_SIZE)
                    .with_context(|| {
                        format!(
                            "failed to read block {} from SRAM bank {bank}",
                            block_offset / STREAM_BLOCK_SIZE
                        )
                    })?;
                out.extend_from_slice(&chunk);
            }
        }

        let disable_result = self.disable_cartridge_ram();
        disable_result.context("failed to disable cartridge RAM after dumping SRAM")?;
        Ok(out)
    }

    pub fn write_sram(&mut self, input: &Path, bank_count: usize, bank_size: usize) -> Result<()> {
        self.ensure_parent_directory(input)?;

        let write_result = self.write_sram_from_file(input, bank_count, bank_size);
        let disable_result = self.disable_cartridge_ram();
        write_result?;
        disable_result.context("failed to disable cartridge RAM after writing SRAM")?;
        Ok(())
    }

    /// Write the provided SRAM bytes directly to the cartridge without using a
    /// temporary file on disk. Expects data.len() == bank_count * bank_size.
    pub fn write_sram_bytes(&mut self, data: &[u8], bank_count: usize, bank_size: usize) -> Result<()> {
        if data.len() != bank_count * bank_size {
            bail!(
                "input SRAM data had unexpected size: expected {} bytes, got {}",
                bank_count * bank_size,
                data.len()
            );
        }

        self.enable_cartridge_ram()?;
        for bank in 0..bank_count {
            self.write_sram_bank(data, bank, bank_count, bank_size)?;
        }

        let disable_result = self.disable_cartridge_ram();
        disable_result.context("failed to disable cartridge RAM after writing SRAM")?;
        Ok(())
    }

    fn write_sram_from_file(
        &mut self,
        input: &Path,
        bank_count: usize,
        bank_size: usize,
    ) -> Result<()> {
        use std::fs;
        let data = fs::read(input)
            .with_context(|| format!("failed to read {}", input.display()))?;
        if data.len() != bank_count * bank_size {
            bail!(
                "input SRAM file had unexpected size: expected {} bytes, got {}",
                bank_count * bank_size,
                data.len()
            );
        }

        self.enable_cartridge_ram()?;
        for bank in 0..bank_count {
            self.write_sram_bank(&data, bank, bank_count, bank_size)?;
        }

        Ok(())
    }

    fn write_sram_bank(
        &mut self,
        data: &[u8],
        bank_index: usize,
        bank_count: usize,
        bank_size: usize,
    ) -> Result<()> {
        let bank = u8::try_from(bank_index).context("SRAM bank index overflowed u8")?;
        debug_log(&format!("writing SRAM bank {bank} of {}", bank_count - 1));
        progress_log(&format!(
            "Writing SRAM bank {}/{}...",
            bank_index + 1,
            bank_count
        ));
        self.select_sram_bank(bank)?;

        let bank_start = bank_index * bank_size;
        for block_offset in (0..bank_size).step_by(STREAM_BLOCK_SIZE) {
            let absolute_start = bank_start + block_offset;
            self.write_sram_block(data, absolute_start, block_offset)?;
        }

        Ok(())
    }

    fn write_sram_block(
        &mut self,
        data: &[u8],
        absolute_start: usize,
        offset_in_bank: usize,
    ) -> Result<()> {
        let end = (absolute_start + STREAM_BLOCK_SIZE).min(data.len());
        let block = &data[absolute_start..end];

        for (i, &b) in block.iter().enumerate() {
            let addr = 0xA000_u16.wrapping_add((offset_in_bank + i) as u16);
            self.write_dmg_cart(addr, b).with_context(|| {
                format!(
                    "failed to write byte {} of block {}",
                    i,
                    offset_in_bank / STREAM_BLOCK_SIZE
                )
            })?;
        }

        Ok(())
    }

    // Helper to read & validate SRAM file and split into bank/block counts for testing.
    #[cfg(test)]
    fn read_and_validate_sram_file(input: &Path, expected_size: usize) -> Result<Vec<u8>> {
        use std::fs;
        let data = fs::read(input)
            .with_context(|| format!("failed to read {}", input.display()))?;
        if data.len() != expected_size {
            bail!(
                "input SRAM file had unexpected size: expected {} bytes, got {}",
                expected_size,
                data.len()
            );
        }
        Ok(data)
    }

    #[cfg(test)]
    fn split_sram_into_blocks<'a>(
        data: &'a [u8],
        bank_count: usize,
        bank_size: usize,
    ) -> Vec<&'a [u8]> {
        let mut blocks = Vec::new();
        for bank in 0..bank_count {
            let bank_start = bank * bank_size;
            for offset in (0..bank_size).step_by(STREAM_BLOCK_SIZE) {
                let start = bank_start + offset;
                let end = std::cmp::min(start + STREAM_BLOCK_SIZE, data.len());
                blocks.push(&data[start..end]);
            }
        }
        blocks
    }

    fn try_connect(port_name: &str, baud_rate: u32, timeout: Duration) -> Result<Self> {
        let mut port = Self::open_port(port_name, baud_rate, timeout)?;
        Self::reset_device_state_for_probe(&mut *port, port_name, baud_rate)?;
        let info = Self::probe_device_info(&mut *port, port_name, baud_rate)?;

        Ok(Self {
            port_name: port_name.to_owned(),
            port,
            info,
        })
    }

    fn uses_modern_firmware(&self) -> bool {
        self.info.firmware_version >= 12
    }

    fn prepare_for_game_boy_camera_modern(&mut self) -> Result<()> {
        debug_log("using modern firmware prep path");
        progress_log("Preparing the reader with the modern firmware path...");
        let power_query = self.request_value(QUERY_CART_POWER_BINARY_COMMAND);
        log_power_query("binary query cart power", &power_query);

        self.configure_modern_game_boy_mode()?;
        if matches!(power_query, Ok(0)) {
            self.power_on_cartridge_binary()?;
        }

        self.finish_game_boy_prep()?;
        self.info.cartridge_mode = CartridgeMode::GameBoy;
        debug_log("modern firmware prep completed");
        Ok(())
    }

    fn prepare_for_game_boy_camera_legacy(&mut self) -> Result<()> {
        progress_log("Preparing the reader with the legacy firmware path...");
        let power_query = self.request_value(QUERY_CART_POWER_COMMAND);
        log_power_query("query cart power", &power_query);

        self.maybe_set_legacy_voltage()?;
        if self.should_power_on_legacy_cartridge(&power_query) {
            self.power_on_cartridge_legacy()?;
        }

        self.enter_legacy_game_boy_mode()?;
        self.finish_game_boy_prep()?;
        self.info.cartridge_mode = self.confirm_cartridge_mode()?;
        debug_log(&format!(
            "confirmed cart mode after prep: {:?}",
            self.info.cartridge_mode
        ));
        Ok(())
    }

    fn configure_modern_game_boy_mode(&mut self) -> Result<()> {
        progress_log("Switching the reader into Game Boy mode...");
        self.send_command_expect_ack(SET_MODE_DMG_COMMAND, "set DMG mode")?;
        progress_log("Setting cartridge voltage to 5V...");
        self.send_command_expect_ack(SET_VOLTAGE_5V_BINARY_COMMAND, "set 5V")?;
        progress_log("Disabling cart pull-ups...");
        self.send_command_expect_ack(DISABLE_PULLUPS_COMMAND, "disable pullups")?;
        self.set_fw_variable(1, FW_VAR_CART_MODE, 1)?;
        self.set_fw_variable(4, FW_VAR_ADDRESS, 0)?;
        Ok(())
    }

    fn power_on_cartridge_binary(&mut self) -> Result<()> {
        progress_log("Powering on the cartridge...");
        self.send_command_expect_ack(CART_POWER_ON_BINARY_COMMAND, "power on cartridge")?;
        thread::sleep(Duration::from_millis(200));
        Ok(())
    }

    fn should_power_on_legacy_cartridge(&self, power_query: &Result<u8>) -> bool {
        match power_query {
            Ok(0) => true,
            Ok(_) => false,
            Err(_) => self.info.firmware_version != 0,
        }
    }

    fn maybe_set_legacy_voltage(&mut self) -> Result<()> {
        if matches!(self.info.pcb_version, PCB_1_3 | PCB_1_4 | PCB_GBXMAS)
            || self.info.firmware_version != 0
        {
            debug_log("sending legacy 5V command");
            progress_log("Setting cartridge voltage to 5V...");
            self.send_command(VOLTAGE_5V_COMMAND)
                .context("failed to switch the GBxCart RW to 5V mode")?;
            self.wait_and_clear_buffers(Duration::from_millis(500));
        }
        Ok(())
    }

    fn power_on_cartridge_legacy(&mut self) -> Result<()> {
        debug_log("sending legacy cart power on command");
        progress_log("Powering on the cartridge...");
        self.send_command(POWER_CART_ON_COMMAND)
            .context("failed to power on the cartridge")?;
        self.wait_and_clear_buffers(Duration::from_millis(500));
        Ok(())
    }

    fn enter_legacy_game_boy_mode(&mut self) -> Result<()> {
        debug_log("switching to DMG mode and resetting mapper");
        progress_log("Switching the reader into Game Boy mode...");
        self.send_command(GB_CART_MODE_COMMAND)
            .context("failed to switch the GBxCart RW into Game Boy cart mode")?;
        self.send_command(SET_MODE_DMG_COMMAND)
            .context("failed to switch the GBxCart RW into binary Game Boy mode")?;
        self.send_command(SET_VOLTAGE_5V_BINARY_COMMAND)
            .context("failed to switch the GBxCart RW into binary 5V mode")?;
        Ok(())
    }

    fn finish_game_boy_prep(&mut self) -> Result<()> {
        progress_log("Resetting the cartridge mapper...");
        self.send_command_expect_ack(DMG_MBC_RESET_COMMAND, "reset DMG mapper")?;
        thread::sleep(Duration::from_millis(150));
        self.clear_buffers()
    }

    fn confirm_cartridge_mode(&mut self) -> Result<CartridgeMode> {
        self.request_value(CART_MODE_COMMAND)
            .context("failed to confirm Game Boy cart mode after switching")?
            .try_into()
            .map_err(|mode: u8| anyhow!("unexpected cart mode response 0x{mode:02X}"))
    }

    fn wait_and_clear_buffers(&mut self, delay: Duration) {
        thread::sleep(delay);
        self.clear_buffers().ok();
    }

    fn ensure_parent_directory(&self, output: &Path) -> Result<()> {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create output directory {}", parent.display())
                })?;
            }
        }

        Ok(())
    }

    fn dump_sram_to_file(
        &mut self,
        output: &Path,
        bank_count: usize,
        bank_size: usize,
    ) -> Result<()> {
        let mut file = File::create(output)
            .with_context(|| format!("failed to create {}", output.display()))?;

        self.enable_cartridge_ram()?;
        for bank in 0..bank_count {
            self.dump_sram_bank(&mut file, bank, bank_count, bank_size)?;
        }

        Ok(())
    }

    fn enable_cartridge_ram(&mut self) -> Result<()> {
        progress_log("Enabling cartridge RAM...");
        self.set_bank(0x0000, 0x0A)
            .context("failed to enable cartridge RAM")
    }

    fn disable_cartridge_ram(&mut self) -> Result<()> {
        progress_log("Disabling cartridge RAM...");
        self.set_bank(0x0000, 0x00)
    }

    fn dump_sram_bank(
        &mut self,
        file: &mut File,
        bank_index: usize,
        bank_count: usize,
        bank_size: usize,
    ) -> Result<()> {
        let bank = u8::try_from(bank_index).context("SRAM bank index overflowed u8")?;
        debug_log(&format!("dumping SRAM bank {bank} of {}", bank_count - 1));
        progress_log(&format!(
            "Reading SRAM bank {}/{}...",
            bank_index + 1,
            bank_count
        ));
        self.select_sram_bank(bank)?;

        for block_offset in (0..bank_size).step_by(STREAM_BLOCK_SIZE) {
            self.dump_sram_block(file, bank, block_offset)?;
        }

        Ok(())
    }

    fn select_sram_bank(&mut self, bank: u8) -> Result<()> {
        self.set_bank(0x4000, bank)
            .with_context(|| format!("failed to select SRAM bank {bank}"))
    }

    fn dump_sram_block(&mut self, file: &mut File, bank: u8, block_offset: usize) -> Result<()> {
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
        })
    }

    fn open_port(
        port_name: &str,
        baud_rate: u32,
        timeout: Duration,
    ) -> Result<Box<dyn SerialPort>> {
        debug_log(&format!("opening {port_name} at {baud_rate} baud"));
        serialport::new(port_name, baud_rate)
            .timeout(timeout)
            .open()
            .with_context(|| format!("failed to open {port_name}"))
    }

    fn reset_device_state_for_probe(
        port: &mut dyn SerialPort,
        port_name: &str,
        baud_rate: u32,
    ) -> Result<()> {
        clear_buffers(port)
            .with_context(|| format!("failed to clear serial buffers on {port_name}"))?;
        debug_log(&format!(
            "sending stop stream to {port_name} at {baud_rate}"
        ));
        send_command(port, STOP_STREAM_COMMAND)
            .with_context(|| format!("failed to reset the device state on {port_name}"))?;
        clear_buffers(port).ok();

        debug_log(&format!("sending reset avr to {port_name} at {baud_rate}"));
        let _ = send_command(port, RESET_AVR_COMMAND);
        thread::sleep(Duration::from_millis(500));
        clear_buffers(port).ok();
        Ok(())
    }

    fn probe_device_info(
        port: &mut dyn SerialPort,
        port_name: &str,
        baud_rate: u32,
    ) -> Result<DeviceInfo> {
        let cart_mode = request_value(port, CART_MODE_COMMAND)
            .with_context(|| format!("failed to probe {port_name}"))?;
        let cartridge_mode = CartridgeMode::from_byte(cart_mode)
            .ok_or_else(|| anyhow!("unexpected cart mode response 0x{cart_mode:02X}"))?;
        let pcb_version = request_value(port, READ_PCB_VERSION_COMMAND)
            .with_context(|| format!("failed to read PCB version from {port_name}"))?;
        let firmware_version = request_value(port, READ_FIRMWARE_VERSION_COMMAND)
            .with_context(|| format!("failed to read firmware version from {port_name}"))?;

        debug_log(&format!(
            "probe {port_name} @ {baud_rate}: cart_mode=0x{cart_mode:02X} pcb=0x{pcb_version:02X} fw=0x{firmware_version:02X}"
        ));

        Ok(DeviceInfo {
            baud_rate,
            cartridge_mode,
            pcb_version,
            firmware_version,
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
        debug_log(&format!(
            "write dmg cart address=0x{address:04X} value=0x{value:02X}"
        ));
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
        debug_log(&format!(
            "read dmg rom address=0x{address:08X} length=0x{length:X}"
        ));
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
        debug_log_verbose(&format!(
            "read dmg ram address=0x{address:04X} length=0x{length:X}"
        ));
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

fn log_power_query(label: &str, result: &Result<u8>) {
    match result {
        Ok(value) => debug_log(&format!("{label} returned 0x{value:02X}")),
        Err(error) => debug_log(&format!("{label} failed: {error:#}")),
    }
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
    use std::fs;

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

    #[test]
    fn read_and_validate_sram_file_rejects_wrong_size() {
        let mut path = std::env::temp_dir();
        path.push(format!("gb-sram-test-{}", std::process::id()));
        let data = vec![0u8; 100];
        fs::write(&path, &data).unwrap();

        let res = GbxcartDevice::read_and_validate_sram_file(&path, 200);
        assert!(res.is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn split_sram_into_blocks_has_expected_counts_and_content() {
        // create data of 2 banks, each bank_size = STREAM_BLOCK_SIZE * 3
        let bank_count = 2;
        let bank_size = STREAM_BLOCK_SIZE * 3;
        let total = bank_count * bank_size;
        let mut data = Vec::with_capacity(total);
        for i in 0..total {
            data.push((i % 256) as u8);
        }

        let blocks = GbxcartDevice::split_sram_into_blocks(&data, bank_count, bank_size);
        // Expect bank_count * 3 blocks
        assert_eq!(blocks.len(), bank_count * 3);

        // verify first block contents
        assert_eq!(blocks[0].len(), STREAM_BLOCK_SIZE);
        for i in 0..STREAM_BLOCK_SIZE {
            assert_eq!(blocks[0][i], (i % 256) as u8);
        }

        // verify last block contents
        let last = blocks.last().unwrap();
        assert_eq!(last.len(), STREAM_BLOCK_SIZE);
        let last_start = total - STREAM_BLOCK_SIZE;
        for i in 0..STREAM_BLOCK_SIZE {
            assert_eq!(last[i], ((last_start + i) % 256) as u8);
        }
    }
}
