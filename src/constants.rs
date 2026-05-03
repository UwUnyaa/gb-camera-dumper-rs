#![allow(dead_code)]
pub(crate) mod camera {
    pub const CARTRIDGE_TITLE: &str = "GAMEBOYCAMERA";
    pub const JAPANESE_CARTRIDGE_TITLE: &str = "POCKETCAMERA";

    pub const SRAM_SIZE: usize = 0x20_000;
    pub const SRAM_BANK_SIZE: usize = 0x2_000;
    pub const SRAM_BANK_COUNT: usize = 16;
    pub const PHOTO_SLOT_COUNT: usize = 30;

    pub const CAPTURE_BUFFER_SIZE: usize = 0x1000;
    pub const GENERAL_DATA_SIZE: usize = 0x00D9;
    pub const STATE_VECTOR_SIZE: usize = 0x0025;
    pub const GAME_FACE_SIZE: usize = 0x0E00;
    pub const CAMERA_TAG_SIZE: usize = 0x0004;
    pub const PHOTO_SLOT_SIZE: usize = 0x1000;

    pub const PHOTO_TILE_DATA_SIZE: usize = 0x0E00;
    pub const PHOTO_THUMBNAIL_SIZE: usize = 0x0100;
    pub const PICTURE_OWNER_METADATA_SIZE: usize = 0x005C;
    pub const PICTURE_OWNER_METADATA_ECHO_SIZE: usize = 0x005C;
    pub const CAMERA_OWNER_METADATA_SIZE: usize = 0x0019;
    pub const CAMERA_OWNER_METADATA_ECHO_SIZE: usize = 0x0019;
    pub const PHOTO_SLOT_TRAILER_SIZE: usize = 0x0016;
}

pub(crate) mod config {
    pub const DEFAULT_OUTPUT_PATH: &str = "gb-camera.sav";
}

pub(crate) mod gbxcart {
    pub const PROBE_TIMEOUT_MS: u64 = 1_000;
    pub const BAUD_RATES: [u32; 3] = [1_000_000, 1_700_000, 1_500_000];
    pub const STREAM_BLOCK_SIZE: usize = 64;
    pub const HEADER_READ_LENGTH: usize = 0x180;
    pub const CART_MODE_COMMAND: u8 = b'C';
    pub const READ_PCB_VERSION_COMMAND: u8 = b'h';
    pub const READ_FIRMWARE_VERSION_COMMAND: u8 = b'V';
    pub const GB_CART_MODE_COMMAND: u8 = b'G';
    pub const STOP_STREAM_COMMAND: u8 = b'0';
    pub const VOLTAGE_5V_COMMAND: u8 = b'5';
    pub const QUERY_CART_POWER_COMMAND: u8 = b']';
    pub const POWER_CART_ON_COMMAND: u8 = b'/';
    pub const RESET_AVR_COMMAND: u8 = b'*';
    pub const CART_POWER_ON_BINARY_COMMAND: u8 = 0xF2;
    pub const QUERY_CART_POWER_BINARY_COMMAND: u8 = 0xF4;
    pub const SET_MODE_DMG_COMMAND: u8 = 0xA3;
    pub const SET_VOLTAGE_5V_BINARY_COMMAND: u8 = 0xA5;
    pub const SET_VARIABLE_COMMAND: u8 = 0xA6;
    pub const DISABLE_PULLUPS_COMMAND: u8 = 0xAC;
    pub const DMG_CART_READ_COMMAND: u8 = 0xB1;
    pub const DMG_CART_WRITE_COMMAND: u8 = 0xB2;
    pub const DMG_MBC_RESET_COMMAND: u8 = 0xB4;

    pub const CART_MODE_GB: u8 = 1;
    pub const CART_MODE_GBA: u8 = 2;
    pub const PCB_1_3: u8 = 4;
    pub const PCB_1_4: u8 = 5;
    pub const PCB_GBXMAS: u8 = 90;
    pub const FW_VAR_ADDRESS: u32 = 0x00;
    pub const FW_VAR_TRANSFER_SIZE: u32 = 0x00;
    pub const FW_VAR_CART_MODE: u32 = 0x00;
    pub const FW_VAR_DMG_ACCESS_MODE: u32 = 0x01;
    pub const FW_VAR_DMG_READ_CS_PULSE: u32 = 0x08;
    pub const DMG_ACCESS_MODE_ROM_READ: u32 = 0x01;
    pub const DMG_ACCESS_MODE_RAM_READ: u32 = 0x03;

    pub const NINTENDO_LOGO: [u8; 48] = [
        0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00,
        0x0D, 0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD,
        0xD9, 0x99, 0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB,
        0xB9, 0x33, 0x3E,
    ];
}
