use anyhow::{Context, Result, ensure};
use chrono::NaiveDateTime;
use png::{BitDepth, ColorType, Encoder};
use std::{
    fs::{self, File},
    io::BufWriter,
    path::Path,
};

use crate::constants::camera::*;
use crate::filename::{PhotoFilenameContext, build_photo_filename};
use crate::log::progress_log;

const STATE_VECTOR_OFFSET: usize = 0x011B2;
const PHOTO_SLOTS_OFFSET: usize = 0x02000;
const EMPTY_ALBUM_SLOT: u8 = 0xFF;
const PHOTO_WIDTH: usize = 128;
const PHOTO_HEIGHT: usize = 112;
const TILE_EDGE: usize = 8;
const TILE_BYTES: usize = 16;
const TILES_PER_ROW: usize = PHOTO_WIDTH / TILE_EDGE;
const GRAYSCALE_PALETTE: [u8; 4] = [0xFF, 0xAA, 0x55, 0x00];

pub fn dump_photos_as_pngs(
    sram: &[u8],
    output_dir: &Path,
    scale: usize,
    filename_template: &str,
    export_time: NaiveDateTime,
    dump_all: bool,
    palette: Option<[u8; 4]>,
) -> Result<usize> {
    ensure!(
        sram.len() == SRAM_SIZE,
        "expected a {}-byte Game Boy Camera SRAM dump, got {} bytes",
        SRAM_SIZE,
        sram.len()
    );
    ensure!(scale > 0, "photo scale must be at least 1");

    if output_dir.exists() {
        progress_log(&format!(
            "Using existing photo output directory {}...",
            output_dir.display()
        ));
    } else {
        progress_log(&format!(
            "Creating photo output directory {}...",
            output_dir.display()
        ));
    }
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "failed to create photo output directory {}",
            output_dir.display()
        )
    })?;

    let photo_slots: Vec<(usize, usize)> = if dump_all {
        // Dump every slot (sequential number == slot index here)
        (0..PHOTO_SLOT_COUNT).map(|i| (i, i)).collect()
    } else {
        active_album_photo_slots(sram)?
    };

    progress_log(&format!(
        "Found {} photos to export.",
        photo_slots.len()
    ));
    for (sequential_index, (_, photo_slot_index)) in photo_slots.iter().enumerate() {
        let output_path = output_dir.join(build_photo_filename(
            filename_template,
            export_time,
            PhotoFilenameContext {
                sequential_number: sequential_index + 1,
                photo_slot_number: photo_slot_index + 1,
            },
        )?);
        progress_log(&format!(
            "Saving photo {}/{} from slot {} to {}...",
            sequential_index + 1,
            photo_slots.len(),
            photo_slot_index + 1,
            output_path.display()
        ));
        dump_photo_slot_as_png(sram, *photo_slot_index, &output_path, scale, palette)?;
    }

    Ok(photo_slots.len())
}

fn active_album_photo_slots(sram: &[u8]) -> Result<Vec<(usize, usize)>> {
    let state_vector = sram
        .get(STATE_VECTOR_OFFSET..STATE_VECTOR_OFFSET + STATE_VECTOR_SIZE)
        .context("SRAM dump did not contain the Game Boy Camera state vector")?;

    let mut active_photos = Vec::new();
    for (album_slot_index, &photo_slot) in state_vector[..PHOTO_SLOT_COUNT].iter().enumerate() {
        if photo_slot == EMPTY_ALBUM_SLOT {
            continue;
        }

        ensure!(
            usize::from(photo_slot) < PHOTO_SLOT_COUNT,
            "album slot {} referenced out-of-range photo slot {}",
            album_slot_index + 1,
            photo_slot
        );
        active_photos.push((album_slot_index, usize::from(photo_slot)));
    }

    Ok(active_photos)
}

fn dump_photo_slot_as_png(
    sram: &[u8],
    photo_slot_index: usize,
    output_path: &Path,
    scale: usize,
    palette: Option<[u8; 4]>,
) -> Result<()> {
    let image_tiles = photo_slot_image_tiles(sram, photo_slot_index)?;
    let palette_ref = palette.unwrap_or(GRAYSCALE_PALETTE);
    let pixels = decode_photo_tiles(image_tiles, &palette_ref);
    let scaled_pixels = scale_grayscale_pixels(&pixels, PHOTO_WIDTH, PHOTO_HEIGHT, scale);
    write_grayscale_png(
        output_path,
        &scaled_pixels,
        scaled_dimension(PHOTO_WIDTH, scale)?,
        scaled_dimension(PHOTO_HEIGHT, scale)?,
    )
}

fn photo_slot_image_tiles(sram: &[u8], photo_slot_index: usize) -> Result<&[u8]> {
    ensure!(
        photo_slot_index < PHOTO_SLOT_COUNT,
        "photo slot index {} was out of range",
        photo_slot_index
    );

    let start = PHOTO_SLOTS_OFFSET + photo_slot_index * PHOTO_SLOT_SIZE;
    sram.get(start..start + PHOTO_TILE_DATA_SIZE)
        .with_context(|| {
            format!(
                "SRAM dump did not contain photo slot {}",
                photo_slot_index + 1
            )
        })
}

fn decode_photo_tiles(tile_data: &[u8], palette: &[u8; 4]) -> Vec<u8> {
    let mut pixels = vec![0; PHOTO_WIDTH * PHOTO_HEIGHT];

    for (tile_index, tile) in tile_data.chunks_exact(TILE_BYTES).enumerate() {
        let tile_x = tile_index % TILES_PER_ROW;
        let tile_y = tile_index / TILES_PER_ROW;

        for row in 0..TILE_EDGE {
            let low_plane = tile[row * 2];
            let high_plane = tile[row * 2 + 1];
            let pixel_y = tile_y * TILE_EDGE + row;

            for column in 0..TILE_EDGE {
                let mask = 1 << (7 - column);
                let shade_index = usize::from(
                    (low_plane & mask != 0) as u8 | (((high_plane & mask) != 0) as u8) << 1,
                );
                let pixel_x = tile_x * TILE_EDGE + column;
                pixels[pixel_y * PHOTO_WIDTH + pixel_x] = palette[shade_index];
            }
        }
    }

    pixels
}

fn scale_grayscale_pixels(pixels: &[u8], width: usize, height: usize, scale: usize) -> Vec<u8> {
    if scale == 1 {
        return pixels.to_vec();
    }

    let scaled_width = width * scale;
    let mut scaled_pixels = vec![0; scaled_width * height * scale];

    for y in 0..height {
        let source_row = &pixels[y * width..(y + 1) * width];

        for row_repeat in 0..scale {
            let scaled_row_start = (y * scale + row_repeat) * scaled_width;
            let scaled_row = &mut scaled_pixels[scaled_row_start..scaled_row_start + scaled_width];

            for (x, &pixel) in source_row.iter().enumerate() {
                let scaled_x = x * scale;
                scaled_row[scaled_x..scaled_x + scale].fill(pixel);
            }
        }
    }

    scaled_pixels
}

fn scaled_dimension(dimension: usize, scale: usize) -> Result<u32> {
    dimension
        .checked_mul(scale)
        .context("scaled photo dimensions overflowed")?
        .try_into()
        .context("scaled photo dimensions did not fit in PNG size fields")
}

fn write_grayscale_png(output_path: &Path, pixels: &[u8], width: u32, height: u32) -> Result<()> {
    let file = File::create(output_path)
        .with_context(|| format!("failed to create {}", output_path.display()))?;
    let writer = BufWriter::new(file);

    let mut encoder = Encoder::new(writer, width, height);
    encoder.set_color(ColorType::Grayscale);
    encoder.set_depth(BitDepth::Eight);

    let mut png_writer = encoder
        .write_header()
        .with_context(|| format!("failed to write PNG header for {}", output_path.display()))?;
    png_writer
        .write_image_data(pixels)
        .with_context(|| format!("failed to write PNG data to {}", output_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use png::Decoder;
    use std::{
        fs,
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn finds_undeleted_photos_from_album_state() {
        let mut sram = vec![0; SRAM_SIZE];
        sram[STATE_VECTOR_OFFSET..STATE_VECTOR_OFFSET + PHOTO_SLOT_COUNT].fill(EMPTY_ALBUM_SLOT);
        sram[STATE_VECTOR_OFFSET] = 2;
        sram[STATE_VECTOR_OFFSET + 2] = 7;

        assert_eq!(
            active_album_photo_slots(&sram).unwrap(),
            vec![(0, 2), (2, 7)]
        );
    }

    #[test]
    fn decodes_game_boy_camera_tiles_to_grayscale_pixels() {
        let mut tile_data = vec![0; PHOTO_TILE_DATA_SIZE];
        tile_data[0] = 0b1100_0000;
        tile_data[1] = 0b0110_0000;

        let pixels = decode_photo_tiles(&tile_data, &GRAYSCALE_PALETTE);

        assert_eq!(
            &pixels[..8],
            &[0xAA, 0x00, 0x55, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn dumps_only_undeleted_photos_to_pngs() {
        let mut sram = vec![0; SRAM_SIZE];
        sram[STATE_VECTOR_OFFSET..STATE_VECTOR_OFFSET + PHOTO_SLOT_COUNT].fill(EMPTY_ALBUM_SLOT);
        sram[STATE_VECTOR_OFFSET] = 0;
        sram[PHOTO_SLOTS_OFFSET] = 0b1000_0000;

        let output_dir = std::env::temp_dir().join(format!(
            "gb-camera-dumper-rs-photo-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        fs::create_dir_all(&output_dir).unwrap();
        let stale_file = output_dir.join("stale.txt");
        fs::write(&stale_file, b"stale").unwrap();

        let export_time = NaiveDate::from_ymd_opt(2026, 5, 4)
            .unwrap()
            .and_hms_opt(13, 7, 0)
            .unwrap();
        let photo_count = dump_photos_as_pngs(
            &sram,
            &output_dir,
            2,
            "photo-{sequential:02}-slot-{slot:02}.png",
            export_time,
            false,
            None,
        )
        .unwrap();
        let png_path = output_dir.join("photo-01-slot-01.png");
        let png_bytes = fs::read(&png_path).unwrap();
        let decoder = Decoder::new(Cursor::new(&png_bytes));
        let reader = decoder.read_info().unwrap();

        assert_eq!(photo_count, 1);
        // Do not clear existing directory contents — the tool should dump into an
        // existing directory without removing stale files.
        assert!(stale_file.exists());
        assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(reader.info().width, (PHOTO_WIDTH * 2) as u32);
        assert_eq!(reader.info().height, (PHOTO_HEIGHT * 2) as u32);

        fs::remove_dir_all(output_dir).unwrap();
    }
}
