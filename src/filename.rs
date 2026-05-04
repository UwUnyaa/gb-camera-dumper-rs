use anyhow::{Result, bail, ensure};
use chrono::{Datelike, NaiveDateTime, Timelike};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhotoFilenameContext {
    pub sequential_number: usize,
    pub photo_slot_number: usize,
}

pub fn build_photo_filename(
    template: &str,
    export_time: NaiveDateTime,
    context: PhotoFilenameContext,
) -> Result<String> {
    let mut filename = String::new();
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    filename.push('{');
                    continue;
                }

                let mut placeholder = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => placeholder.push(c),
                        None => bail!("filename template contains an unmatched '{{'"),
                    }
                }

                filename.push_str(&render_placeholder(&placeholder, export_time, context)?);
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    filename.push('}');
                } else {
                    bail!("filename template contains an unmatched '}}'");
                }
            }
            _ => filename.push(ch),
        }
    }

    ensure!(
        !filename.is_empty(),
        "filename template must produce a non-empty file name"
    );
    ensure!(
        !filename.contains('/') && !filename.contains('\\'),
        "filename template must not include path separators"
    );

    Ok(filename)
}

fn render_placeholder(
    placeholder: &str,
    export_time: NaiveDateTime,
    context: PhotoFilenameContext,
) -> Result<String> {
    let (field, width) = parse_placeholder(placeholder)?;
    let numeric_value = match field {
        "year" => export_time.year() as usize,
        "month" => export_time.month() as usize,
        "day" => export_time.day() as usize,
        "hour24" => export_time.hour() as usize,
        "hour12" => {
            let hour = export_time.hour() % 12;
            if hour == 0 { 12 } else { hour as usize }
        }
        "minute" => export_time.minute() as usize,
        "sequential" => context.sequential_number,
        "slot" => context.photo_slot_number,
        _ => bail!(
            "filename template uses unsupported field {{{}}}; supported fields are year, month, day, hour24, hour12, minute, sequential, and slot",
            placeholder
        ),
    };

    Ok(match width {
        Some(width) => format!("{numeric_value:0width$}"),
        None => numeric_value.to_string(),
    })
}

fn parse_placeholder(placeholder: &str) -> Result<(&str, Option<usize>)> {
    let mut parts = placeholder.splitn(2, ':');
    let field = parts.next().unwrap_or_default();
    ensure!(
        !field.is_empty(),
        "filename template contains an empty field"
    );

    let width = match parts.next() {
        Some(specifier) => {
            ensure!(
                !specifier.is_empty(),
                "filename template field {{{placeholder}}} has an empty width specifier"
            );
            ensure!(
                specifier.chars().all(|ch| ch.is_ascii_digit()),
                "filename template field {{{placeholder}}} has an invalid width specifier"
            );

            Some(specifier.parse::<usize>().map_err(|_| {
                anyhow::anyhow!(
                    "filename template field {{{placeholder}}} has an invalid width specifier"
                )
            })?)
        }
        None => None,
    };

    Ok((field, width))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn sample_export_time() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 5, 4)
            .unwrap()
            .and_hms_opt(13, 7, 0)
            .unwrap()
    }

    fn sample_context() -> PhotoFilenameContext {
        PhotoFilenameContext {
            sequential_number: 3,
            photo_slot_number: 12,
        }
    }

    #[test]
    fn fills_date_and_photo_number_fields() {
        let filename = build_photo_filename(
            "{year}-{month:02}-{day:02}_{hour24:02}-{minute:02}_photo-{sequential:02}_slot-{slot:02}.png",
            sample_export_time(),
            sample_context(),
        )
        .unwrap();

        assert_eq!(filename, "2026-05-04_13-07_photo-03_slot-12.png");
    }

    #[test]
    fn renders_12_hour_clock_values() {
        let filename = build_photo_filename(
            "{hour12:02}-{minute:02}.png",
            sample_export_time(),
            sample_context(),
        )
        .unwrap();

        assert_eq!(filename, "01-07.png");
    }

    #[test]
    fn supports_escaped_braces() {
        let filename = build_photo_filename(
            "{{photo}}-{slot}.png",
            sample_export_time(),
            sample_context(),
        )
        .unwrap();

        assert_eq!(filename, "{photo}-12.png");
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = build_photo_filename("{album}.png", sample_export_time(), sample_context())
            .unwrap_err();

        assert!(error.to_string().contains("unsupported field {album}"));
    }

    #[test]
    fn rejects_path_separators() {
        let error =
            build_photo_filename("nested/{slot}.png", sample_export_time(), sample_context())
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must not include path separators")
        );
    }
}
