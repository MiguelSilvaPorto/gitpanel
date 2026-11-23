#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use gitpanel_i18n::LanguagePreference;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSettings {
    pub theme: ThemePreference,
    pub language: LanguagePreference,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            language: LanguagePreference::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSettings {
    pub interval_minutes: u64,
    pub backfill_days: u32,
    pub sync_all_accessible: bool,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            interval_minutes: 15,
            backfill_days: 90,
            sync_all_accessible: true,
        }
    }
}

impl SyncSettings {
    pub fn normalize(&mut self) {
        self.interval_minutes = self.interval_minutes.clamp(5, 1440);
        self.backfill_days = self.backfill_days.clamp(1, 3650);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Settings {
    pub ui: UiSettings,
    pub sync: SyncSettings,
}

impl Settings {
    pub fn normalize(&mut self) {
        self.sync.normalize();
    }
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub config_file: PathBuf,
    pub database_file: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self, ConfigError> {
        let dirs = ProjectDirs::from("io", "MiguelSilvaPorto", "gitpanel")
            .ok_or(ConfigError::NoProjectDirs)?;
        let config_dir = dirs.config_dir().to_path_buf();
        let data_dir = dirs.data_local_dir().to_path_buf();
        let cache_dir = dirs.cache_dir().to_path_buf();
        Ok(Self {
            config_file: config_dir.join("config.toml"),
            database_file: data_dir.join("gitpanel.db"),
            config_dir,
            data_dir,
            cache_dir,
        })
    }

    pub fn ensure_dirs(&self) -> Result<(), ConfigError> {
        secure_dir(&self.config_dir)?;
        secure_dir(&self.data_dir)?;
        secure_dir(&self.cache_dir)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub paths: Paths,
    pub settings: Settings,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let paths = Paths::discover()?;
        Self::load_with_paths(paths)
    }

    pub fn load_with_paths(paths: Paths) -> Result<Self, ConfigError> {
        paths.ensure_dirs()?;
        let mut settings = if paths.config_file.is_file() {
            let source =
                fs::read_to_string(&paths.config_file).map_err(|source| ConfigError::Io {
                    path: paths.config_file.clone(),
                    source,
                })?;
            toml::from_str::<Settings>(&source).map_err(|source| ConfigError::Parse {
                path: paths.config_file.clone(),
                source,
            })?
        } else {
            Settings::default()
        };
        settings.normalize();
        Ok(Self { paths, settings })
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.paths.ensure_dirs()?;
        let source = toml::to_string_pretty(&self.settings).map_err(ConfigError::Serialize)?;
        let temporary = self.paths.config_file.with_extension("toml.tmp");
        fs::write(&temporary, source).map_err(|source| ConfigError::Io {
            path: temporary.clone(),
            source,
        })?;
        secure_file(&temporary)?;
        fs::rename(&temporary, &self.paths.config_file).map_err(|source| ConfigError::Io {
            path: self.paths.config_file.clone(),
            source,
        })?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the application directories")]
    NoProjectDirs,
    #[error("could not read or write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not serialize settings: {0}")]
    Serialize(#[source] toml::ser::Error),
}

fn secure_dir(path: &Path) -> Result<(), ConfigError> {
    fs::create_dir_all(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
            ConfigError::Io {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

fn secure_file(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
            ConfigError::Io {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            config_file: root.join("config/config.toml"),
            database_file: root.join("data/gitpanel.db"),
        }
    }

    #[test]
    fn creates_defaults_and_roundtrips() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(temp.path());
        let mut config = Config::load_with_paths(paths.clone()).expect("load");
        assert_eq!(config.settings, Settings::default());
        config.settings.ui.theme = ThemePreference::Dark;
        config.settings.ui.language = LanguagePreference::Locale("pt-BR".to_owned());
        config.settings.sync.interval_minutes = 30;
        config.save().expect("save");
        let loaded = Config::load_with_paths(paths).expect("reload");
        assert_eq!(loaded.settings.ui.theme, ThemePreference::Dark);
        assert_eq!(loaded.settings.sync.interval_minutes, 30);
    }

    #[test]
    fn clamps_sync_values() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = paths(temp.path());
        fs::create_dir_all(&paths.config_dir).expect("config dir");
        fs::write(
            &paths.config_file,
            "[ui]\ntheme = \"dark\"\nlanguage = \"pt-BR\"\n\n[sync]\ninterval_minutes = 1\nbackfill_days = 99999\nsync_all_accessible = true\n",
        )
        .expect("write");
        let config = Config::load_with_paths(paths).expect("load");
        assert_eq!(config.settings.sync.interval_minutes, 5);
        assert_eq!(config.settings.sync.backfill_days, 3650);
    }
}
