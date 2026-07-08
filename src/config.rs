use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub inbox_dir: PathBuf,
    pub library_root: PathBuf,
    pub database_url: String,
    #[serde(default)]
    pub grobid_url: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        cfg.inbox_dir = expand_tilde(cfg.inbox_dir, home.clone());
        cfg.library_root = expand_tilde(cfg.library_root, home);
        Ok(cfg)
    }
}

/// Expand a leading `~/` (or bare `~`) using `home`; otherwise return as-is.
fn expand_tilde(p: PathBuf, home: Option<PathBuf>) -> PathBuf {
    match (p.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_minimal_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();

        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.inbox_dir, PathBuf::from("/data/inbox"));
        assert_eq!(cfg.library_root, PathBuf::from("/data/library"));
        assert_eq!(cfg.database_url, "sqlite:/data/library.db");
        assert_eq!(cfg.grobid_url, None);
    }

    #[test]
    fn expands_leading_tilde_with_home() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            expand_tilde(PathBuf::from("~/papers/inbox"), home.clone()),
            PathBuf::from("/home/u/papers/inbox")
        );
        // No tilde, or no HOME: unchanged.
        assert_eq!(
            expand_tilde(PathBuf::from("/data/inbox"), home),
            PathBuf::from("/data/inbox")
        );
        assert_eq!(
            expand_tilde(PathBuf::from("~/x"), None),
            PathBuf::from("~/x")
        );
    }

    #[test]
    fn load_error_names_the_file() {
        let err = Config::load(Path::new("/nope/xuewen.toml")).unwrap_err();
        assert!(err.to_string().contains("/nope/xuewen.toml"));
    }
}
