use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, PartialEq, Eq, Default)]
pub struct Config {
    /// Optional path to write the dumped SRAM file (e.g. "out.sav").
    pub output_path: Option<String>,

    /// Directory where exported photos will be written (relative or absolute).
    pub photo_output_dir: Option<String>,

    /// Filename template for exported photos. When omitted, DEFAULT_FILENAME_TEMPLATE is used.
    pub filename_template: Option<String>,

    /// If true, dump all photo slots including those marked deleted; otherwise only active photos are exported.
    pub dump_all_photos: Option<bool>,

    /// If true, mark photos as deleted on the cartridge after dumping them.
    pub mark_deleted_after_dump: Option<bool>,

    /// Optional custom 4-color palette as hex strings (e.g. ["#000000","#555555","#AAAAAA","#FFFFFF"]).
    /// If omitted, a default grayscale palette is applied by the program.
    pub palette: Option<[String; 4]>,

    /// Image scale multiplier (1 = original size). Use integers like 1, 2, etc. Default is 1.
    pub image_scale: Option<u32>,
}

/// Returns an XDG-compliant config path when XDG_CONFIG_HOME is set:
///   $XDG_CONFIG_HOME/gb-camera-dumper/config.yaml
/// Otherwise falls back to: $HOME/.gb-camera-dumper-config.yaml
pub fn get_config_path() -> PathBuf {
    get_config_path_for(None, None)
}

/// Like get_config_path but allows overriding the XDG and HOME values for
/// testing or callers that want to supply explicit paths.
pub fn get_config_path_for(xdg: Option<&str>, home: Option<&str>) -> PathBuf {
    if let Some(xdg) = xdg {
        let mut p = PathBuf::from(xdg);
        p.push("gb-camera-dumper");
        p.push("config.yaml");
        return p;
    }
    if let Some(home) = home {
        return PathBuf::from(home).join(".gb-camera-dumper-config.yaml");
    }
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("gb-camera-dumper");
        p.push("config.yaml");
        return p;
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".gb-camera-dumper-config.yaml");
    }
    PathBuf::from(".gb-camera-dumper-config.yaml")
}

/// Read and parse the YAML config file at `path` into a Config struct.
pub fn parse_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let path = path.as_ref();
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    let cfg: Config = serde_yaml::from_str(&data)
        .with_context(|| format!("failed to parse YAML in {}", path.display()))?;
    Ok(cfg)
}

/// Load the config from the standard config path (XDG or HOME fallback).
/// If the config file is missing, prints a message and returns the default Config.
pub fn load_config() -> Result<Config> {
    load_config_for(None, None)
}

/// Load the config from an explicit path if provided, otherwise fall back to
/// the standard load_config behaviour. If the explicit path is provided but
/// the file does not exist, a message is printed and defaults are returned.
pub fn load_config_from_path<P: AsRef<Path>>(path_opt: Option<P>) -> Result<Config> {
    if let Some(p) = path_opt {
        let p = p.as_ref();
        if !p.exists() {
            println!("Config file not found at {}; using defaults.", p.display());
            return Ok(Config::default());
        }
        return parse_config(p);
    }
    load_config()
}

/// Like load_config but allows overriding the XDG and HOME values for testing.
pub fn load_config_for(xdg: Option<&str>, home: Option<&str>) -> Result<Config> {
    let path = get_config_path_for(xdg, home);
    if !path.exists() {
        // Print a stable message that references XDG_CONFIG_HOME and HOME regardless of env.
        let xdg_ref = "$XDG_CONFIG_HOME/gb-camera-dumper/config.yaml";
        let home_ref = "$HOME/.gb-camera-dumper-config.yaml";
        println!("Config file not found; looked at {} or {}; using defaults.", xdg_ref, home_ref);
        return Ok(Config::default());
    }
    parse_config(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_get_config_path_xdg() {
        let p = get_config_path_for(Some("/tmp/mycfgdir"), None);
        assert!(p.ends_with(PathBuf::from("gb-camera-dumper").join("config.yaml")));
    }

    #[test]
    fn test_get_config_path_home_fallback() {
        let p = get_config_path_for(None, Some("/home/testuser"));
        assert_eq!(p, PathBuf::from("/home/testuser/.gb-camera-dumper-config.yaml"));
    }

    #[test]
    fn test_parse_config() {
        let mut path = std::env::temp_dir();
        path.push(format!("gb-camera-dumper-test-{}.yaml", std::process::id()));
        let yaml = "output_path: \"out.sav\"\nphoto_output_dir: \"photos\"\nfilename_template: \"tmpl\"\ndump_all_photos: true\nmark_deleted_after_dump: false\npalette: [\"#000000\", \"#555555\", \"#AAAAAA\", \"#FFFFFF\"]\nimage_scale: 2\n";
        fs::write(&path, yaml).unwrap();
        let cfg = parse_config(&path).unwrap();
        assert_eq!(cfg.output_path.unwrap(), "out.sav");
        assert_eq!(cfg.photo_output_dir.unwrap(), "photos");
        assert_eq!(cfg.filename_template.unwrap(), "tmpl");
        assert_eq!(cfg.dump_all_photos.unwrap(), true);
        assert_eq!(cfg.mark_deleted_after_dump.unwrap(), false);
        assert_eq!(cfg.palette.unwrap(), ["#000000".to_string(), "#555555".to_string(), "#AAAAAA".to_string(), "#FFFFFF".to_string()]);
        assert_eq!(cfg.image_scale.unwrap(), 2);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_load_config_missing() {
        // Point home to a unique temp path that doesn't contain a config file.
        let temp_home = std::env::temp_dir().join(format!("gbcfg-home-{}", std::process::id()));
        let h = temp_home.to_str().unwrap();
        let cfg = load_config_for(None, Some(h)).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn test_load_config_present_xdg() {
        // Create an XDG-style config: $XDG_CONFIG_HOME/gb-camera-dumper/config.yaml
        let tempdir = std::env::temp_dir().join(format!("gbcfg-xdg-{}", std::process::id()));
        let gbdir = tempdir.join("gb-camera-dumper");
        fs::create_dir_all(&gbdir).unwrap();
        let cfgfile = gbdir.join("config.yaml");
        let yaml = "dump_all_photos: true\nimage_scale: 3\n";
        fs::write(&cfgfile, yaml).unwrap();
        let cfg = load_config_for(Some(tempdir.to_str().unwrap()), None).unwrap();
        assert_eq!(cfg.dump_all_photos.unwrap(), true);
        assert_eq!(cfg.image_scale.unwrap(), 3);
        let _ = fs::remove_file(&cfgfile);
        let _ = fs::remove_dir(&gbdir);
    }
}
