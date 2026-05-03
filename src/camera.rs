#![allow(dead_code)]

// Cartridge metadata
pub const CARTRIDGE_TITLE: &str = "GAMEBOYCAMERA";
pub const JAPANESE_CARTRIDGE_TITLE: &str = "POCKETCAMERA";

// SRAM layout for the retail Game Boy Camera / Pocket Camera cartridges.
//
// Layout sources:
// - Pan Docs, "Game Boy Camera" for the cartridge RAM size and banking model:
//   https://gbdev.io/pandocs/Gameboy_Camera.html
// - Raphael Boichot, "All you want to know about the Game Boy Camera save format !"
//   for the reverse-engineered absolute SRAM offsets.

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

pub fn is_game_boy_camera_title(title: &str) -> bool {
    matches!(title, CARTRIDGE_TITLE | JAPANESE_CARTRIDGE_TITLE)
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct GameBoyCameraSram {
    /// 0x00000..=0x00FFF: 128x128 capture buffer / exchange buffer.
    pub capture_buffer: [u8; CAPTURE_BUFFER_SIZE],
    /// 0x01000..=0x010D8: animation settings, minigames, counters, and checksum.
    pub general_data: GeneralDataBlock,
    /// 0x010D9..=0x011B1: echo of `general_data`.
    pub general_data_echo: GeneralDataBlock,
    /// 0x011B2..=0x011D6: album slot numbering and checksum.
    pub state_vector: StateVectorBlock,
    /// 0x011D7..=0x011FB: echo of `state_vector`.
    pub state_vector_echo: StateVectorBlock,
    /// 0x011FC..=0x01FFB: 128x112 Game Face image.
    pub game_face: [u8; GAME_FACE_SIZE],
    /// 0x01FFC..=0x01FFF: optional camera tag, e.g. CoroCoro unlock bytes.
    pub camera_tag: [u8; CAMERA_TAG_SIZE],
    /// 0x02000..=0x1FFFF: 30 photo slots of 0x1000 bytes each.
    pub photo_slots: [PhotoSlot; PHOTO_SLOT_COUNT],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct GeneralDataBlock {
    /// 0x00..=0x2E: animation slot list.
    pub animation_slots: [u8; 0x2F],
    /// 0x2F: animation loop flag.
    pub animation_loop_flag: u8,
    /// 0x30..=0x5E: animation loop definitions.
    pub animation_loops: [u8; 0x2F],
    /// 0x5F: animation speed.
    pub animation_speed: u8,
    /// 0x60: animation border.
    pub animation_border: u8,
    /// 0x61..=0x74: DJ sound I settings and note data.
    pub sound_i: [u8; 0x14],
    /// 0x75..=0x78: DJ sound I stereo options.
    pub sound_i_stereo: [u8; 0x04],
    /// 0x79..=0x88: DJ sound II wave envelope.
    pub sound_ii_wave: [u8; 0x10],
    /// 0x89..=0x9B: DJ sound II settings and note data.
    pub sound_ii: [u8; 0x13],
    /// 0x9C..=0x9F: DJ sound II stereo options.
    pub sound_ii_stereo: [u8; 0x04],
    /// 0xA0: loop count for the two sound channels.
    pub loop_count: u8,
    /// 0xA1..=0xB6: DJ noise settings and note data.
    pub noise: [u8; 0x16],
    /// 0xB7..=0xB8: unknown / apparently unused bytes.
    pub unknown_b7_b8: [u8; 0x02],
    /// 0xB9: tempo.
    pub tempo: u8,
    /// 0xBA: nonzero when a song has been saved.
    pub partition_saved_flag: u8,
    /// 0xBB..=0xBC: picture counter.
    pub pictures_taken: [u8; 0x02],
    /// 0xBD..=0xBE: erase counter.
    pub pictures_erased: [u8; 0x02],
    /// 0xBF..=0xC0: transfer counter.
    pub pictures_transferred: [u8; 0x02],
    /// 0xC1..=0xC2: printer counter.
    pub pictures_printed: [u8; 0x02],
    /// 0xC3..=0xC4: received pictures by male/female.
    pub pictures_received: [u8; 0x02],
    /// 0xC5..=0xC8: Space Fever II score.
    pub score_space_fever: [u8; 0x04],
    /// 0xC9..=0xCA: Ball score.
    pub score_ball: [u8; 0x02],
    /// 0xCB..=0xCC: Run! Run! Run! score storage.
    pub score_run_run_run: [u8; 0x02],
    /// 0xCD..=0xCF: unknown bytes.
    pub unknown_cd_cf: [u8; 0x03],
    /// 0xD0: printer intensity.
    pub printing_intensity: u8,
    /// 0xD1: unknown byte.
    pub unknown_d1: u8,
    /// 0xD2..=0xD6: ASCII "Magic".
    pub magic: [u8; 0x05],
    /// 0xD7..=0xD8: checksum bytes.
    pub checksum: [u8; 0x02],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct StateVectorBlock {
    /// 0x00..=0x1D: album order / slot state for 30 stored pictures.
    pub album_picture_order: [u8; PHOTO_SLOT_COUNT],
    /// 0x1E..=0x22: ASCII "Magic".
    pub magic: [u8; 0x05],
    /// 0x23..=0x24: checksum bytes.
    pub checksum: [u8; 0x02],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct PhotoSlot {
    /// 0x000..=0xDFF: main 128x112 2bpp image tiles.
    pub image_tiles: [u8; PHOTO_TILE_DATA_SIZE],
    /// 0xE00..=0xEFF: 32x32 thumbnail tiles.
    pub thumbnail_tiles: [u8; PHOTO_THUMBNAIL_SIZE],
    /// 0xF00..=0xF5B: picture owner metadata and checksum.
    pub picture_owner_metadata: PictureOwnerMetadata,
    /// 0xF5C..=0xFB7: echo of `picture_owner_metadata`.
    pub picture_owner_metadata_echo: [u8; PICTURE_OWNER_METADATA_ECHO_SIZE],
    /// 0xFB8..=0xFD0: camera owner metadata for slot 1, usually 0xAA in others.
    pub camera_owner_metadata: CameraOwnerMetadata,
    /// 0xFD1..=0xFE9: echo of `camera_owner_metadata`.
    pub camera_owner_metadata_echo: [u8; CAMERA_OWNER_METADATA_ECHO_SIZE],
    /// 0xFEA..=0xFFF: trailing bytes, usually 0xAA.
    ///
    /// Reverse-engineering notes indicate that calibration bytes may appear in
    /// the trailing region of the slots covering absolute ranges
    /// 0x04FF2..=0x04FFF and 0x11FF2..=0x11FFF.
    pub trailer: [u8; PHOTO_SLOT_TRAILER_SIZE],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct PictureOwnerMetadata {
    /// 0x00..=0x03: picture owner ID.
    pub user_id: [u8; 0x04],
    /// 0x04..=0x0C: picture owner name.
    pub user_name: [u8; 0x09],
    /// 0x0D: gender and blood type bitfield.
    pub gender_and_blood_type: u8,
    /// 0x0E..=0x11: packed birth date digits.
    pub birth_date: [u8; 0x04],
    /// 0x12..=0x14: unknown bytes.
    pub unknown_12_14: [u8; 0x03],
    /// 0x15..=0x2F: comment text.
    pub comment: [u8; 0x1B],
    /// 0x30..=0x32: zero bytes.
    pub zero_30_32: [u8; 0x03],
    /// 0x33: copy/original flag.
    pub copy_flag: u8,
    /// 0x34..=0x35: image checksum or change-detection bytes.
    pub image_checksum_hint: [u8; 0x02],
    /// 0x36..=0x3A: hotspot enabled flags.
    pub hotspot_enabled: [u8; 0x05],
    /// 0x3B..=0x3F: hotspot X coordinates.
    pub hotspot_x: [u8; 0x05],
    /// 0x40..=0x44: hotspot Y coordinates.
    pub hotspot_y: [u8; 0x05],
    /// 0x45..=0x49: hotspot sound/music selections.
    pub hotspot_sound: [u8; 0x05],
    /// 0x4A..=0x4E: hotspot visual effects.
    pub hotspot_effect: [u8; 0x05],
    /// 0x4F..=0x53: hotspot jump targets.
    pub hotspot_jump_target: [u8; 0x05],
    /// 0x54: border number.
    pub border: u8,
    /// 0x55..=0x59: ASCII "Magic".
    pub magic: [u8; 0x05],
    /// 0x5A..=0x5B: checksum bytes.
    pub checksum: [u8; 0x02],
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct CameraOwnerMetadata {
    /// 0x00..=0x03: camera owner ID.
    pub user_id: [u8; 0x04],
    /// 0x04..=0x0C: camera owner name.
    pub user_name: [u8; 0x09],
    /// 0x0D: gender and blood type bitfield.
    pub gender_and_blood_type: u8,
    /// 0x0E..=0x11: packed birth date digits.
    pub birth_date: [u8; 0x04],
    /// 0x12..=0x16: ASCII "Magic".
    pub magic: [u8; 0x05],
    /// 0x17..=0x18: checksum bytes.
    pub checksum: [u8; 0x02],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn block_sizes_match_reverse_engineered_layout() {
        assert_eq!(SRAM_BANK_SIZE * SRAM_BANK_COUNT, SRAM_SIZE);
        assert_eq!(size_of::<GeneralDataBlock>(), GENERAL_DATA_SIZE);
        assert_eq!(size_of::<StateVectorBlock>(), STATE_VECTOR_SIZE);
        assert_eq!(
            size_of::<PictureOwnerMetadata>(),
            PICTURE_OWNER_METADATA_SIZE
        );
        assert_eq!(size_of::<CameraOwnerMetadata>(), CAMERA_OWNER_METADATA_SIZE);
        assert_eq!(size_of::<PhotoSlot>(), PHOTO_SLOT_SIZE);
        assert_eq!(size_of::<GameBoyCameraSram>(), SRAM_SIZE);
    }

    #[test]
    fn recognises_supported_camera_titles() {
        assert!(is_game_boy_camera_title(CARTRIDGE_TITLE));
        assert!(is_game_boy_camera_title(JAPANESE_CARTRIDGE_TITLE));
        assert!(!is_game_boy_camera_title("TETRIS"));
    }
}
