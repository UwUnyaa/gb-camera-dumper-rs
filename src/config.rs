use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, PartialEq, Eq, Default)]
pub struct Config {
    pub output_path: Option<String>,
    pub photo_output_dir: Option<String>,
    pub filename_template: Option<String>,
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
        let yaml = "output_path: \"out.sav\"\nphoto_output_dir: \"photos\"\nfilename_template: \"tmpl\"\n";
        fs::write(&path, yaml).unwrap();
        let cfg = parse_config(&path).unwrap();
        assert_eq!(cfg.output_path.unwrap(), "out.sav");
        assert_eq!(cfg.photo_output_dir.unwrap(), "photos");
        assert_eq!(cfg.filename_template.unwrap(), "tmpl");
        let _ = fs::remove_file(&path);
    }
}
