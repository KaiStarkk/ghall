use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

/// All available columns for the repos table
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
    Origin,
    Repository,
    Type,
    Updated,
    Archived,
    Private,
    Ghq,
    Status,
    Dirty,
    Path,
}

impl Column {
    /// All columns in default order
    pub fn default_order() -> Vec<Column> {
        vec![
            Column::Origin,
            Column::Repository,
            Column::Type,
            Column::Updated,
            Column::Archived,
            Column::Private,
            Column::Ghq,
            Column::Status,
            Column::Dirty,
            Column::Path,
        ]
    }

    /// Get display name for the column
    pub fn name(&self) -> &'static str {
        match self {
            Column::Origin => "Origin",
            Column::Repository => "Repository",
            Column::Type => "Type",
            Column::Updated => "Updated",
            Column::Archived => "Arch",
            Column::Private => "Priv",
            Column::Ghq => "ghq?",
            Column::Status => "Status",
            Column::Dirty => "Dirty",
            Column::Path => "Path",
        }
    }

    /// Get column width constraint (includes room for sort indicator [Name ▲])
    pub fn width(&self) -> u16 {
        match self {
            Column::Origin => 18,      // [Origin ▲]
            Column::Repository => 24,  // [Repository ▲]
            Column::Type => 20,        // [Type ▲]
            Column::Updated => 16,     // [Updated ▲]
            Column::Archived => 10,    // [Arch ▲]
            Column::Private => 10,     // [Priv ▲]
            Column::Ghq => 10,         // [ghq? ▲]
            Column::Status => 14,      // [Status ▲]
            Column::Dirty => 11,       // [Dirty ▲]
            Column::Path => 0,         // Min constraint, takes remainder
        }
    }

}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// IDs of ignored/hidden repositories
    #[serde(default)]
    pub ignored_repos: HashSet<String>,

    /// Visible columns in display order
    #[serde(default = "Column::default_order")]
    pub columns: Vec<Column>,

    /// Default sort column name
    #[serde(default = "default_sort_column")]
    pub sort_column: String,

    /// Sort ascending by default
    #[serde(default)]
    pub sort_ascending: bool,

    /// Show archived repos
    #[serde(default = "default_true")]
    pub show_archived: bool,

    /// Show private repos
    #[serde(default = "default_true")]
    pub show_private: bool,
}

fn default_sort_column() -> String {
    "updated".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ignored_repos: HashSet::new(),
            columns: Column::default_order(),
            sort_column: default_sort_column(),
            sort_ascending: false,
            show_archived: true,
            show_private: true,
        }
    }
}

impl Config {
    /// Get the config directory path
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ghall")
    }

    /// Get the config file path
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Load config from file, falling back to defaults
    pub fn load() -> Self {
        let path = Self::config_path();

        // Try to load from TOML
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str(&content) {
                return config;
            }
        }

        // Check for legacy ignored.txt file and migrate
        let legacy_path = Self::config_dir().join("ignored.txt");
        if legacy_path.exists() {
            let mut config = Config::default();
            if let Ok(content) = fs::read_to_string(&legacy_path) {
                config.ignored_repos = content.lines().map(|s| s.to_string()).collect();
            }
            // Save migrated config
            config.save();
            // Remove legacy file
            let _ = fs::remove_file(legacy_path);
            return config;
        }

        Config::default()
    }

    /// Save config to file
    pub fn save(&self) {
        let dir = Self::config_dir();
        if fs::create_dir_all(&dir).is_ok() {
            let path = Self::config_path();
            if let Ok(content) = toml::to_string_pretty(self) {
                let _ = fs::write(path, content);
            }
        }
    }

    /// Move selected column left
    pub fn move_column_left(&mut self, col: Column) {
        if let Some(idx) = self.columns.iter().position(|&c| c == col) {
            if idx > 0 {
                self.columns.swap(idx, idx - 1);
            }
        }
    }

    /// Move selected column right
    pub fn move_column_right(&mut self, col: Column) {
        if let Some(idx) = self.columns.iter().position(|&c| c == col) {
            if idx < self.columns.len() - 1 {
                self.columns.swap(idx, idx + 1);
            }
        }
    }
}
