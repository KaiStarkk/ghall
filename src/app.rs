use crate::config::{Column, Config};
use crate::git::RepoStatus;
use crate::{git, github, local};
use anyhow::Result;
use chrono::Local;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tokio::sync::mpsc;

/// An entry in the error log
#[derive(Debug, Clone)]
pub struct ErrorLogEntry {
    pub timestamp: String,
    pub operation: String,
    pub error: String,
}

impl ErrorLogEntry {
    pub fn new(operation: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            operation: operation.into(),
            error: error.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Repos,
    Gists,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortColumn {
    Origin,
    Name,
    Type,
    Status,
    LastUpdated,
    Path,
    Dirty,
    Private,
    Archived,
    Ghq,
}

impl SortColumn {
    /// Get next sort column based on visible columns
    pub fn next(self, visible: &[Column]) -> Self {
        let current_col = self.to_column();
        if let Some(idx) = visible.iter().position(|&c| c == current_col) {
            let next_idx = (idx + 1) % visible.len();
            Self::from_column(visible[next_idx])
        } else if !visible.is_empty() {
            Self::from_column(visible[0])
        } else {
            self
        }
    }

    /// Get previous sort column based on visible columns
    pub fn prev(self, visible: &[Column]) -> Self {
        let current_col = self.to_column();
        if let Some(idx) = visible.iter().position(|&c| c == current_col) {
            let prev_idx = if idx == 0 { visible.len() - 1 } else { idx - 1 };
            Self::from_column(visible[prev_idx])
        } else if !visible.is_empty() {
            Self::from_column(visible[0])
        } else {
            self
        }
    }

    /// Convert to Column enum
    pub fn to_column(self) -> Column {
        match self {
            SortColumn::Origin => Column::Origin,
            SortColumn::Name => Column::Repository,
            SortColumn::Type => Column::Type,
            SortColumn::Status => Column::Status,
            SortColumn::LastUpdated => Column::Updated,
            SortColumn::Path => Column::Path,
            SortColumn::Dirty => Column::Dirty,
            SortColumn::Private => Column::Private,
            SortColumn::Archived => Column::Archived,
            SortColumn::Ghq => Column::Ghq,
        }
    }

    /// Convert from Column enum
    pub fn from_column(col: Column) -> Self {
        match col {
            Column::Origin => SortColumn::Origin,
            Column::Repository => SortColumn::Name,
            Column::Type => SortColumn::Type,
            Column::Status => SortColumn::Status,
            Column::Updated => SortColumn::LastUpdated,
            Column::Path => SortColumn::Path,
            Column::Dirty => SortColumn::Dirty,
            Column::Private => SortColumn::Private,
            Column::Archived => SortColumn::Archived,
            Column::Ghq => SortColumn::Ghq,
        }
    }

    /// Convert from config string
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "origin" => SortColumn::Origin,
            "repository" | "name" => SortColumn::Name,
            "type" => SortColumn::Type,
            "status" => SortColumn::Status,
            "updated" | "lastupdated" => SortColumn::LastUpdated,
            "path" => SortColumn::Path,
            "dirty" => SortColumn::Dirty,
            "private" | "priv" => SortColumn::Private,
            "archived" | "arch" => SortColumn::Archived,
            "ghq" => SortColumn::Ghq,
            _ => SortColumn::LastUpdated,
        }
    }

    /// Convert to config string
    pub fn as_str(&self) -> &'static str {
        match self {
            SortColumn::Origin => "origin",
            SortColumn::Name => "repository",
            SortColumn::Type => "type",
            SortColumn::Status => "status",
            SortColumn::LastUpdated => "updated",
            SortColumn::Path => "path",
            SortColumn::Dirty => "dirty",
            SortColumn::Private => "private",
            SortColumn::Archived => "archived",
            SortColumn::Ghq => "ghq",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    ConfirmDelete,
    UploadForm,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeleteType {
    LocalRepo,
    RemoteRepo,
    Gist,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PopupType {
    Help,
    Details,
    Ignored,
    Upload,
    Errors,
}

/// Fields in the upload form
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UploadField {
    Name,
    Description,
    Private,
    Org,
}

impl UploadField {
    pub fn next(self) -> Self {
        match self {
            UploadField::Name => UploadField::Description,
            UploadField::Description => UploadField::Private,
            UploadField::Private => UploadField::Org,
            UploadField::Org => UploadField::Name,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            UploadField::Name => UploadField::Org,
            UploadField::Description => UploadField::Name,
            UploadField::Private => UploadField::Description,
            UploadField::Org => UploadField::Private,
        }
    }
}

/// State for the upload form
#[derive(Debug, Clone)]
pub struct UploadFormState {
    pub name: String,
    pub description: String,
    pub private: bool,
    pub orgs: Vec<String>,        // Available orgs
    pub selected_org: usize,      // 0 = personal, 1+ = org index
    pub active_field: UploadField,
    pub local_path: String,       // Path to upload from
}

#[derive(Debug, Clone)]
pub struct Popup {
    pub popup_type: PopupType,
    pub scroll: usize,
    pub content: Vec<String>,
    pub selected: usize,
}

impl Popup {
    pub fn new(popup_type: PopupType, content: Vec<String>) -> Self {
        Self {
            popup_type,
            scroll: 0,
            content,
            selected: 0,
        }
    }

    pub fn scroll_down(&mut self, visible_lines: usize) {
        let max_scroll = self.content.len().saturating_sub(visible_lines);
        self.scroll = (self.scroll + 1).min(max_scroll);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
}

#[derive(Debug, Clone)]
pub struct RepoRow {
    pub id: String,
    pub owner: Option<String>,
    pub name: String,
    pub github_url: Option<String>,
    #[allow(dead_code)]
    pub ssh_url: Option<String>,
    pub is_fork: bool,
    pub fork_parent: Option<String>,
    pub is_private: bool,
    pub is_archived: bool,
    pub is_member: bool, // User owns or is member of org
    pub local_path: Option<String>,
    pub git_status: Option<RepoStatus>,
    pub last_commit_time: Option<i64>, // Unix timestamp
    pub is_subrepo: bool,              // Nested inside another repo
    pub parent_repo: Option<String>,   // Path to parent repo if subrepo
    pub fork_ahead: Option<u32>,       // Commits ahead of upstream (for forks)
    pub fork_behind: Option<u32>,      // Commits behind upstream (for forks)
    pub has_git: bool,                 // Whether this folder has a git repo
}

impl RepoRow {
    pub fn has_local(&self) -> bool {
        self.local_path.is_some()
    }

    pub fn is_local_only(&self) -> bool {
        self.local_path.is_some() && self.github_url.is_none()
    }

    pub fn is_remote_only(&self) -> bool {
        self.github_url.is_some() && self.local_path.is_none()
    }

    pub fn fork_owner(&self) -> Option<&str> {
        self.fork_parent.as_ref().and_then(|p| p.split('/').next())
    }

    /// Returns the expected ghq-style path for this repo
    pub fn expected_ghq_path(&self, local_root: &str) -> Option<String> {
        if let Some(ref owner) = self.owner {
            // Canonicalize local_root to get consistent path
            let root = std::path::Path::new(local_root)
                .canonicalize()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| local_root.trim_end_matches('/').to_string());
            Some(format!("{}/github.com/{}/{}", root, owner, self.name))
        } else {
            None
        }
    }

    /// Checks if the current local path follows ghq convention
    /// Subrepos are always considered to follow ghq (they're nested in their parent)
    pub fn follows_ghq(&self, local_root: &str) -> Option<bool> {
        // Subrepos are always considered as following ghq - they're nested inside
        // their parent repo which should be organized correctly
        if self.is_subrepo {
            return Some(true);
        }

        if let (Some(ref local_path), Some(ref owner)) = (&self.local_path, &self.owner) {
            // Check if path matches pattern: {root}/github.com/{owner}/{name}
            // Use case-insensitive comparison and resolve symlinks
            let local = std::path::Path::new(local_path);

            // Canonicalize the actual local path
            let path_to_check = match local.canonicalize() {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => local_path.clone(),
            };

            // Canonicalize local_root
            let root_canonical = match std::path::Path::new(local_root).canonicalize() {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => local_root.trim_end_matches('/').to_string(),
            };

            // Build expected path and canonicalize it too
            let expected_raw = format!("{}/github.com/{}/{}", root_canonical, owner, self.name);
            let expected = std::path::Path::new(&expected_raw)
                .canonicalize()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(expected_raw);

            // Compare paths (case-insensitive for owner/name on case-insensitive filesystems)
            Some(path_to_check.eq_ignore_ascii_case(&expected) || path_to_check == expected)
        } else {
            None // No local path or no GitHub info
        }
    }
}

#[derive(Debug, Clone)]
pub struct GistRow {
    pub id: String,
    pub description: String,
    pub is_public: bool,
    pub file_names: Vec<String>,
    pub html_url: String,
    pub local_path: Option<String>,
    pub git_status: Option<RepoStatus>,
    #[allow(dead_code)]
    pub created_at: Option<String>,
    #[allow(dead_code)]
    pub updated_at: Option<String>,
}

impl GistRow {
    pub fn has_local(&self) -> bool {
        self.local_path.is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.git_status.as_ref().map(|s| s.is_dirty()).unwrap_or(false)
    }
}

/// Braille spinner frames
pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Cached GitHub data to avoid re-fetching for local-only operations
pub struct GitHubCache {
    pub repos: Vec<github::GitHubRepoInfo>,
    pub gists: Vec<GistRow>,
}

pub struct App {
    pub local_root: String,
    pub view_mode: ViewMode,
    pub github_username: Option<String>,

    // Data
    pub repos: Vec<RepoRow>,
    pub gists: Vec<GistRow>,

    // Configuration (includes ignored_repos, columns, etc.)
    pub config: Config,

    // Selection and sorting
    pub selected: usize,
    pub scroll_offset: usize,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub show_archived: bool,
    pub show_private: bool,

    // Column selection for reordering (index into visible columns)
    pub selected_column: usize,

    // UI state
    pub status_message: Option<String>,
    pub status_time: Option<Instant>,
    pub status_is_loading: bool, // true = show spinner, false = show tick
    pub status_is_error: bool,   // true = show error (red, persistent)
    pub input_mode: InputMode,
    pub popup: Option<Popup>,
    pub input_buffer: String,
    pub confirm_buffer: String,
    pub pending_delete: Option<DeleteType>,

    // Table area for mouse click detection (y offset, height)
    pub table_area: Option<(u16, u16)>,

    // Spinner state for async operations
    pub spinner_frame: usize,

    // Background task communication
    pub task_rx: mpsc::Receiver<TaskResult>,
    pub task_tx: mpsc::Sender<TaskResult>,
    pub refresh_rx: mpsc::Receiver<RefreshData>,
    pub refresh_tx: mpsc::Sender<RefreshData>,
    pub pending_refresh: bool,       // Full refresh (clears cache)
    pub pending_local_refresh: bool, // Local-only refresh (uses cache)

    // GitHub data cache (to avoid re-fetching for local-only operations)
    pub github_cache: Option<GitHubCache>,

    // Upload form state
    pub upload_form: Option<UploadFormState>,

    // Error log for viewing after quit
    pub error_log: Vec<ErrorLogEntry>,
}

/// Result from a background task
pub struct TaskResult {
    pub success: bool,
    pub message: String,
    pub stderr: Option<String>,          // Full stderr for error log
    pub operation: String,               // Operation name for error log
    pub invalidates_github_cache: bool,  // If true, needs full refresh; if false, local-only refresh
}

/// Data loaded from a refresh operation
pub struct RefreshData {
    pub github_username: Option<String>,
    pub repos: Vec<RepoRow>,
    pub gists: Vec<GistRow>,
    pub error: Option<String>,                      // Error message to display in status bar
    pub github_cache: Option<GitHubCache>,          // Cache to store for local-only refreshes
}

/// Perform a full data refresh (runs in background task)
async fn perform_refresh(local_root: String) -> RefreshData {
    // Check gh authentication first
    if let Err(e) = github::check_auth().await {
        // Still discover local repos even without GitHub auth
        let local_repos = local::discover_repos(&local_root).await.unwrap_or_default();
        let repos = merge_repos(Vec::new(), local_repos);
        return RefreshData {
            github_username: None,
            repos,
            gists: Vec::new(),
            error: Some(e.to_string()),
            github_cache: None,
        };
    }

    // Fetch GitHub username
    let github_username = github::get_current_user().await.ok();

    // Fetch GitHub repos via GraphQL
    let mut github_repos = github::fetch_all_repos_graphql().await.unwrap_or_default();

    // Fetch fork comparison data (commits ahead/behind upstream)
    github::fetch_fork_comparisons(&mut github_repos).await;

    // Discover local repos
    let local_repos = local::discover_repos(&local_root).await.unwrap_or_default();

    // Merge into unified list
    let repos = merge_repos(github_repos.clone(), local_repos);

    // Fetch gists
    let gists = github::fetch_gists_as_rows(&local_root).await.unwrap_or_default();

    RefreshData {
        github_username,
        repos,
        github_cache: Some(GitHubCache {
            repos: github_repos,
            gists: gists.clone(),
        }),
        gists,
        error: None,
    }
}

/// Perform a local-only refresh using cached GitHub data (runs in background task)
async fn perform_local_refresh(local_root: String, cache: GitHubCache) -> RefreshData {
    // Discover local repos
    let local_repos = local::discover_repos(&local_root).await.unwrap_or_default();

    // Merge with cached GitHub data
    let repos = merge_repos(cache.repos.clone(), local_repos);

    RefreshData {
        github_username: None, // Keep existing, don't update
        repos,
        gists: cache.gists.clone(),
        error: None,
        github_cache: Some(cache), // Preserve the cache
    }
}

impl App {
    pub fn new(local_root: String) -> Result<Self> {
        // Load config from XDG config
        let config = Config::load();

        // Create channel for background task results
        let (task_tx, task_rx) = mpsc::channel(32);
        let (refresh_tx, refresh_rx) = mpsc::channel(1);

        // Initialize settings from config
        let sort_column = SortColumn::from_string(&config.sort_column);
        let sort_ascending = config.sort_ascending;
        let show_archived = config.show_archived;
        let show_private = config.show_private;

        let app = Self {
            local_root: local_root.clone(),
            view_mode: ViewMode::Repos,
            github_username: None, // Will be fetched during first refresh
            repos: Vec::new(),
            gists: Vec::new(),
            config,
            selected: 0,
            scroll_offset: 0,
            sort_column,
            sort_ascending,
            show_archived,
            show_private,
            selected_column: 0,
            status_message: Some("Loading...".to_string()),
            status_time: Some(Instant::now()),
            status_is_loading: true,
            status_is_error: false,
            input_mode: InputMode::Normal,
            popup: None,
            input_buffer: String::new(),
            confirm_buffer: String::new(),
            pending_delete: None,
            table_area: None,
            spinner_frame: 0,
            task_rx,
            task_tx,
            refresh_rx,
            refresh_tx: refresh_tx.clone(),
            pending_refresh: false,
            pending_local_refresh: false,
            github_cache: None,
            upload_form: None,
            error_log: Vec::new(),
        };

        // Spawn initial refresh in background
        tokio::spawn(async move {
            let refresh_data = perform_refresh(local_root).await;
            let _ = refresh_tx.send(refresh_data).await;
        });

        Ok(app)
    }

    // Check if current user can modify repo visibility
    pub fn can_change_visibility(&self, repo: &RepoRow) -> bool {
        // Can change visibility if user owns or is member of org that owns the repo
        repo.github_url.is_some() && repo.is_member
    }

    /// Trigger a full background refresh (non-blocking, clears cache)
    pub fn trigger_refresh(&mut self) {
        self.set_status("Refreshing...");
        self.github_cache = None; // Clear cache for full refresh
        let local_root = self.local_root.clone();
        let tx = self.refresh_tx.clone();

        tokio::spawn(async move {
            let refresh_data = perform_refresh(local_root).await;
            let _ = tx.send(refresh_data).await;
        });
    }

    /// Trigger a local-only refresh using cached GitHub data (non-blocking)
    /// Falls back to full refresh if no cache is available
    pub fn trigger_local_refresh(&mut self) {
        if let Some(cache) = self.github_cache.take() {
            self.set_status("Updating...");
            let local_root = self.local_root.clone();
            let tx = self.refresh_tx.clone();

            tokio::spawn(async move {
                let refresh_data = perform_local_refresh(local_root, cache).await;
                let _ = tx.send(refresh_data).await;
            });
        } else {
            // No cache available, fall back to full refresh
            self.trigger_refresh();
        }
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Repos => ViewMode::Gists,
            ViewMode::Gists => ViewMode::Repos,
        };
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn next_sort_column(&mut self) {
        self.sort_column = self.sort_column.next(&self.config.columns);
        self.config.sort_column = self.sort_column.as_str().to_string();
        self.config.save();
        self.sort_repos();
    }

    pub fn prev_sort_column(&mut self) {
        self.sort_column = self.sort_column.prev(&self.config.columns);
        self.config.sort_column = self.sort_column.as_str().to_string();
        self.config.save();
        self.sort_repos();
    }

    pub fn toggle_show_archived(&mut self) {
        self.show_archived = !self.show_archived;
        self.config.show_archived = self.show_archived;
        self.config.save();
        self.selected = 0;
    }

    pub fn toggle_show_private(&mut self) {
        self.show_private = !self.show_private;
        self.config.show_private = self.show_private;
        self.config.save();
        self.selected = 0;
    }

    fn sort_repos(&mut self) {
        let username = self.github_username.clone();
        let sort_col = self.sort_column;
        let ascending = self.sort_ascending;
        let local_root = self.local_root.clone();

        self.repos.sort_by(|a, b| {
            let cmp = match sort_col {
                SortColumn::Origin => {
                    let a_owner = a.owner.as_deref().unwrap_or("~");
                    let b_owner = b.owner.as_deref().unwrap_or("~");
                    a_owner.to_lowercase().cmp(&b_owner.to_lowercase())
                }
                SortColumn::Name => {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
                SortColumn::Type => {
                    // Sort by: src (owned) < clone < fork < local
                    let a_type = repo_type_sort_order(a, &username);
                    let b_type = repo_type_sort_order(b, &username);
                    a_type.cmp(&b_type)
                }
                SortColumn::Status => {
                    // Sort by: dirty < diverged < ahead < behind < synced < no-local
                    let a_status = status_sort_order(a);
                    let b_status = status_sort_order(b);
                    a_status.cmp(&b_status)
                }
                SortColumn::LastUpdated => {
                    // Sort by time, None goes last
                    match (a.last_commit_time, b.last_commit_time) {
                        (Some(a_time), Some(b_time)) => a_time.cmp(&b_time),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                }
                SortColumn::Path => {
                    let a_path = a.local_path.as_deref().unwrap_or("~");
                    let b_path = b.local_path.as_deref().unwrap_or("~");
                    a_path.cmp(b_path)
                }
                SortColumn::Dirty => {
                    // Sort dirty repos first
                    let a_dirty = a.git_status.as_ref().map(|s| s.is_dirty()).unwrap_or(false);
                    let b_dirty = b.git_status.as_ref().map(|s| s.is_dirty()).unwrap_or(false);
                    b_dirty.cmp(&a_dirty) // Reverse so dirty comes first
                }
                SortColumn::Private => {
                    // Sort private repos first
                    b.is_private.cmp(&a.is_private)
                }
                SortColumn::Archived => {
                    // Sort archived repos first
                    b.is_archived.cmp(&a.is_archived)
                }
                SortColumn::Ghq => {
                    // Sort by ghq compliance: non-compliant first, then compliant, then N/A
                    let a_ghq = a.follows_ghq(&local_root);
                    let b_ghq = b.follows_ghq(&local_root);
                    match (a_ghq, b_ghq) {
                        (Some(false), Some(true)) => std::cmp::Ordering::Less,
                        (Some(true), Some(false)) => std::cmp::Ordering::Greater,
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
                }
            };
            // Apply ascending/descending
            let primary = if ascending { cmp } else { cmp.reverse() };
            // Use repo ID as tie-breaker for stable sorting
            if primary == std::cmp::Ordering::Equal {
                a.id.cmp(&b.id)
            } else {
                primary
            }
        });
    }

    pub fn visible_repos(&self) -> Vec<&RepoRow> {
        self.repos
            .iter()
            .filter(|r| !self.config.ignored_repos.contains(&r.id))
            .filter(|r| self.show_archived || !r.is_archived)
            .filter(|r| self.show_private || !r.is_private)
            .collect()
    }

    fn visible_list_len(&self) -> usize {
        match self.view_mode {
            ViewMode::Repos => self.visible_repos().len(),
            ViewMode::Gists => self.gists.len(),
        }
    }

    pub fn next(&mut self) {
        let count = self.visible_list_len();
        if count > 0 {
            self.selected = (self.selected + 1).min(count - 1);
        }
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Advance the spinner frame and check for status message timeout
    pub fn tick_spinner(&mut self) {
        if self.status_message.is_some() {
            // Only animate spinner if we're in loading state
            if self.status_is_loading {
                self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
            }

            // Clear status message after 2 seconds if not loading and not an error
            // Error messages persist until user takes action
            if !self.status_is_loading && !self.status_is_error {
                if let Some(time) = self.status_time {
                    if time.elapsed().as_secs() >= 2 {
                        self.status_message = None;
                        self.status_time = None;
                    }
                }
            }
        }
    }

    /// Check for completed background tasks (non-blocking)
    pub fn poll_tasks(&mut self) {
        while let Ok(result) = self.task_rx.try_recv() {
            // Handle special messages
            if result.message.starts_with("__ORGS__:") {
                let orgs_str = result.message.trim_start_matches("__ORGS__:");
                if let Some(ref mut form) = self.upload_form {
                    form.orgs = orgs_str.split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                }
                continue;
            }

            // Log errors with full stderr
            if !result.success {
                if let Some(stderr) = result.stderr {
                    if !stderr.is_empty() {
                        self.error_log.push(ErrorLogEntry::new(&result.operation, &stderr));
                    }
                }
            }

            // Set status as completed (will show tick instead of spinner)
            self.set_status_completed(result.message.clone());

            // Choose refresh type based on whether GitHub cache needs invalidation
            if result.invalidates_github_cache {
                self.pending_refresh = true;
            } else {
                self.pending_local_refresh = true;
            }
        }
    }

    /// Check for completed refresh data (non-blocking)
    pub fn poll_refresh(&mut self) {
        while let Ok(data) = self.refresh_rx.try_recv() {
            // Update app state with refreshed data
            if data.github_username.is_some() {
                self.github_username = data.github_username;
            }
            self.repos = data.repos;
            self.gists = data.gists;

            // Store GitHub cache for local-only refreshes
            if data.github_cache.is_some() {
                self.github_cache = data.github_cache;
            }

            // Re-apply user's sort settings
            self.sort_repos();

            // Clamp selection to valid range
            let max = self.visible_list_len().saturating_sub(1);
            if self.selected > max {
                self.selected = max;
            }

            // Show error if auth failed, otherwise show success
            if let Some(error) = data.error {
                self.set_status_error(error);
            } else {
                self.set_status_completed(format!("Loaded {} repos", self.repos.len()));
            }
        }
    }

    /// Show error log popup
    pub fn show_error_log(&mut self) {
        if self.error_log.is_empty() {
            self.set_status("No errors logged");
            return;
        }

        let content: Vec<String> = self.error_log.iter().flat_map(|e| {
            vec![
                format!("[{}] {}", e.timestamp, e.operation),
                e.error.clone(),
                String::new(),
            ]
        }).collect();

        self.popup = Some(Popup::new(PopupType::Errors, content));
    }

    /// Get error count for status bar
    pub fn error_count(&self) -> usize {
        self.error_log.len()
    }

    /// Get the current spinner character
    pub fn spinner_char(&self) -> char {
        SPINNER_FRAMES[self.spinner_frame]
    }

    /// Set a status message for loading operations (shows spinner)
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_time = Some(Instant::now());
        self.status_is_loading = true;
    }

    /// Set a status message for completed operations (shows tick, auto-clears)
    pub fn set_status_completed(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_time = Some(Instant::now());
        self.status_is_loading = false;
        self.status_is_error = false;
    }

    /// Set a status message for errors (shows X, persistent, red)
    pub fn set_status_error(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
        self.status_time = Some(Instant::now());
        self.status_is_loading = false;
        self.status_is_error = true;
    }

    /// Clear status message
    #[allow(dead_code)]
    pub fn clear_status(&mut self) {
        self.status_message = None;
        self.status_time = None;
        self.status_is_loading = false;
        self.status_is_error = false;
    }

    /// Toggle sort direction
    pub fn toggle_sort_direction(&mut self) {
        self.sort_ascending = !self.sort_ascending;
        self.config.sort_ascending = self.sort_ascending;
        self.config.save();
        self.sort_repos();
    }

    /// Move selected column left
    pub fn move_column_left(&mut self) {
        if let Some(col) = self.config.columns.get(self.selected_column).copied() {
            self.config.move_column_left(col);
            if self.selected_column > 0 {
                self.selected_column -= 1;
            }
            self.config.save();
        }
    }

    /// Move selected column right
    pub fn move_column_right(&mut self) {
        if let Some(col) = self.config.columns.get(self.selected_column).copied() {
            self.config.move_column_right(col);
            if self.selected_column < self.config.columns.len() - 1 {
                self.selected_column += 1;
            }
            self.config.save();
        }
    }

    /// Select next column (for reordering)
    pub fn select_next_column(&mut self) {
        if !self.config.columns.is_empty() {
            self.selected_column = (self.selected_column + 1) % self.config.columns.len();
        }
    }

    /// Select previous column (for reordering)
    pub fn select_prev_column(&mut self) {
        if !self.config.columns.is_empty() {
            if self.selected_column == 0 {
                self.selected_column = self.config.columns.len() - 1;
            } else {
                self.selected_column -= 1;
            }
        }
    }

    /// Get visible columns
    pub fn visible_columns(&self) -> &[Column] {
        &self.config.columns
    }

    /// Get selected column index
    pub fn selected_column_index(&self) -> usize {
        self.selected_column
    }

    /// Copy popup content to clipboard
    pub fn copy_popup_to_clipboard(&mut self) {
        if let Some(ref popup) = self.popup {
            let content = popup.content.join("\n");
            // Try wl-copy first (Wayland), then xclip (X11)
            let result = std::process::Command::new("wl-copy")
                .arg(&content)
                .status()
                .or_else(|_| {
                    std::process::Command::new("xclip")
                        .args(["-selection", "clipboard"])
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .and_then(|mut child| {
                            use std::io::Write;
                            if let Some(stdin) = child.stdin.as_mut() {
                                stdin.write_all(content.as_bytes())?;
                            }
                            child.wait()
                        })
                });

            match result {
                Ok(status) if status.success() => {
                    self.set_status("Copied to clipboard");
                }
                _ => {
                    self.set_status("Failed to copy (install wl-copy or xclip)");
                }
            }
        }
    }

    /// Select a specific row by index (for mouse clicks)
    pub fn select_row(&mut self, row: usize) {
        let count = self.visible_list_len();
        if count > 0 && row < count {
            self.selected = row;
        }
    }

    /// Handle mouse click at position, returning true if it hit the table
    pub fn handle_mouse_click(&mut self, row: u16, _col: u16) -> bool {
        if let Some((table_y, table_height)) = self.table_area {
            // Account for border (1) and header (1) = 2 rows offset
            let header_offset = 2u16;
            if row >= table_y + header_offset && row < table_y + table_height {
                let clicked_row = (row - table_y - header_offset) as usize;
                self.select_row(clicked_row);
                return true;
            }
        }
        false
    }

    pub fn scroll_down(&mut self) {
        if let Some(ref mut popup) = self.popup {
            popup.scroll_down(20);
        }
    }

    pub fn scroll_up(&mut self) {
        if let Some(ref mut popup) = self.popup {
            popup.scroll_up();
        }
    }

    pub fn get_selected_repo(&self) -> Option<&RepoRow> {
        if self.view_mode == ViewMode::Repos {
            self.visible_repos().get(self.selected).copied()
        } else {
            None
        }
    }

    pub fn get_selected_gist(&self) -> Option<&GistRow> {
        if self.view_mode == ViewMode::Gists {
            self.gists.get(self.selected)
        } else {
            None
        }
    }

    pub fn toggle_help(&mut self) {
        if self.popup.is_some() {
            self.popup = None;
        } else {
            self.popup = Some(Popup::new(PopupType::Help, get_help_content(&self.view_mode)));
        }
    }

    pub fn close_popup(&mut self) {
        self.popup = None;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.confirm_buffer.clear();
    }

    // Show details popup for selected item
    pub fn show_details(&mut self) {
        match self.view_mode {
            ViewMode::Repos => {
                if let Some(repo) = self.get_selected_repo() {
                    let mut content = vec![
                        format!("Name: {}", repo.name),
                        format!("Owner: {}", repo.owner.as_deref().unwrap_or("(local)")),
                        "".to_string(),
                    ];

                    if let Some(ref url) = repo.github_url {
                        content.push(format!("GitHub: {}", url));
                    }
                    if let Some(ref path) = repo.local_path {
                        content.push(format!("Local: {}", path));
                    }

                    content.push("".to_string());

                    if repo.is_fork {
                        content.push(format!("Fork of: {}", repo.fork_parent.as_deref().unwrap_or("unknown")));
                    }
                    if repo.is_subrepo {
                        content.push(format!("Subrepo of: {}", repo.parent_repo.as_deref().unwrap_or("unknown")));
                    }
                    content.push(format!("Private: {}", if repo.is_private { "yes" } else { "no" }));

                    if let Some(ref status) = repo.git_status {
                        content.push("".to_string());
                        content.push("Git Status:".to_string());
                        content.push(format!("  Branch: {}", status.branch));
                        if status.has_remote {
                            content.push(format!("  Ahead: {}, Behind: {}", status.ahead, status.behind));
                        } else {
                            content.push("  No upstream configured".to_string());
                        }
                        if status.is_dirty() {
                            content.push(format!("  Dirty: {} staged, {} untracked", status.staged, status.untracked));
                        }
                    }

                    self.popup = Some(Popup::new(PopupType::Details, content));
                }
            }
            ViewMode::Gists => {
                if let Some(gist) = self.get_selected_gist() {
                    let mut content = vec![
                        format!("Description: {}", gist.description),
                        format!("ID: {}", gist.id),
                        format!("Public: {}", if gist.is_public { "yes" } else { "no" }),
                        "".to_string(),
                        format!("URL: {}", gist.html_url),
                    ];

                    if let Some(ref path) = gist.local_path {
                        content.push(format!("Local: {}", path));
                    }

                    content.push("".to_string());
                    content.push(format!("Files ({}):", gist.file_names.len()));
                    for file in &gist.file_names {
                        content.push(format!("  {}", file));
                    }

                    self.popup = Some(Popup::new(PopupType::Details, content));
                }
            }
        }
    }

    // Toggle ignore for selected repo
    pub fn toggle_ignore(&mut self) {
        if let Some(repo) = self.get_selected_repo() {
            let id = repo.id.clone();
            if self.config.ignored_repos.contains(&id) {
                self.config.ignored_repos.remove(&id);
            } else {
                self.config.ignored_repos.insert(id);
                // Adjust selection if needed
                let max = self.visible_list_len().saturating_sub(1);
                if self.selected > max {
                    self.selected = max;
                }
            }
            // Save to config
            self.config.save();
        }
    }

    // Show ignored repos popup
    pub fn show_ignored_popup(&mut self) {
        if self.config.ignored_repos.is_empty() {
            self.popup = Some(Popup::new(PopupType::Ignored, vec!["No ignored repositories.".to_string()]));
        } else {
            let mut content: Vec<String> = self.config.ignored_repos.iter().cloned().collect();
            content.sort();
            content.insert(0, "Ignored Repositories (press Enter to unhide):".to_string());
            content.insert(1, "".to_string());
            self.popup = Some(Popup::new(PopupType::Ignored, content));
        }
    }

    // Unhide selected repo in ignored popup
    pub fn unhide_selected_in_popup(&mut self) {
        if let Some(ref popup) = self.popup {
            if popup.popup_type == PopupType::Ignored && popup.selected >= 2 {
                let idx = popup.selected - 2; // Account for header lines
                let ignored_list: Vec<String> = self.config.ignored_repos.iter().cloned().collect();
                if let Some(id) = ignored_list.get(idx) {
                    self.config.ignored_repos.remove(id);
                    // Save to config
                    self.config.save();
                }
                // Refresh popup
                self.show_ignored_popup();
            }
        }
    }

    // Git operations for selected repo (spawned as background tasks)
    pub fn pull_selected(&mut self) {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.set_status(format!("Pulling {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("pull {}", name);
            tokio::spawn(async move {
                let result = git::pull(&path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Pulled {}", name)
                    } else {
                        "Pull failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    pub fn push_selected(&mut self) {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.set_status(format!("Pushing {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("push {}", name);
            tokio::spawn(async move {
                let result = git::push(&path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Pushed {}", name)
                    } else {
                        "Push failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    pub fn sync_selected(&mut self) {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.set_status(format!("Syncing {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("sync {}", name);
            tokio::spawn(async move {
                let fetch_res = git::fetch(&path).await;
                let pull_res = git::pull(&path).await;
                let push_res = git::push(&path).await;
                let success = fetch_res.success && pull_res.success && push_res.success;
                let stderr = if !success {
                    let mut errs = Vec::new();
                    if !fetch_res.stderr.is_empty() { errs.push(fetch_res.stderr); }
                    if !pull_res.stderr.is_empty() { errs.push(pull_res.stderr); }
                    if !push_res.stderr.is_empty() { errs.push(push_res.stderr); }
                    Some(errs.join("\n"))
                } else {
                    None
                };
                let _ = tx.send(TaskResult {
                    success,
                    message: if success {
                        format!("Synced {}", name)
                    } else {
                        "Sync failed (E: view errors)".to_string()
                    },
                    stderr,
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    /// Quicksync: fetch, ff-rebase, add all, commit with fixup, push
    pub fn quicksync_selected(&mut self) {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.set_status(format!("Quicksyncing {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("quicksync {}", name);
            tokio::spawn(async move {
                let result = git::quicksync(&path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Quicksynced {}", name)
                    } else {
                        "Quicksync failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    pub fn clone_selected(&mut self) {
        let info = self.get_selected_repo().and_then(|r| {
            if r.is_remote_only() {
                // Use HTTPS URL for cloning (works with gh CLI auth)
                r.github_url.clone().map(|url| (r.name.clone(), url))
            } else {
                None
            }
        });
        if let Some((name, url)) = info {
            let clone_path = get_ghq_path(&self.local_root, &url);
            self.set_status(format!("Cloning {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("clone {}", name);
            tokio::spawn(async move {
                let result = git::clone(&url, &clone_path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Cloned {}", name)
                    } else {
                        "Clone failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Clone creates local copy, doesn't change GitHub
                }).await;
            });
        }
    }

    pub fn init_repo(&mut self) {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone(), r.has_git));
        if let Some((name, Some(path), false)) = info {
            self.set_status(format!("Initializing git repo in {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("init {}", name);
            tokio::spawn(async move {
                let result = git::init(&path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Initialized git repo in {}", name)
                    } else {
                        "Git init failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Local filesystem operation
                }).await;
            });
        }
    }

    pub fn start_delete_confirm(&mut self) {
        let has_local = self.get_selected_repo()
            .map(|r| r.has_local())
            .unwrap_or(false);
        if has_local {
            self.input_mode = InputMode::ConfirmDelete;
            self.pending_delete = Some(DeleteType::LocalRepo);
            self.confirm_buffer.clear();
        }
    }

    pub fn start_delete_remote_confirm(&mut self) {
        let can_delete = self.get_selected_repo()
            .map(|r| r.github_url.is_some() && r.is_member)
            .unwrap_or(false);
        if can_delete {
            self.input_mode = InputMode::ConfirmDelete;
            self.pending_delete = Some(DeleteType::RemoteRepo);
            self.confirm_buffer.clear();
        }
    }

    pub fn delete_local_repo(&mut self) {
        if self.confirm_buffer.to_lowercase() == "y" || self.confirm_buffer.to_lowercase() == "yes" {
            let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
            if let Some((name, Some(path))) = info {
                self.set_status(format!("Deleting {}...", name));
                let tx = self.task_tx.clone();
                let op = format!("delete local {}", name);
                tokio::spawn(async move {
                    let result = tokio::fs::remove_dir_all(&path).await;
                    let _ = tx.send(TaskResult {
                        success: result.is_ok(),
                        message: if result.is_ok() {
                            format!("Deleted {}", name)
                        } else {
                            format!("Failed to delete {}", name)
                        },
                        stderr: result.err().map(|e| e.to_string()),
                        operation: op,
                        invalidates_github_cache: false, // Local filesystem operation
                    }).await;
                });
                self.close_popup();
            }
        } else {
            self.close_popup();
        }
        self.pending_delete = None;
    }

    pub fn delete_remote_repo(&mut self) {
        if self.confirm_buffer.to_lowercase() == "y" || self.confirm_buffer.to_lowercase() == "yes" {
            let info = self.get_selected_repo().and_then(|r| {
                r.owner.clone().map(|o| format!("{}/{}", o, r.name))
            });
            if let Some(name_with_owner) = info {
                self.set_status(format!("Deleting remote {}...", name_with_owner));
                let tx = self.task_tx.clone();
                let name = name_with_owner.clone();
                let op = format!("delete remote {}", name);
                tokio::spawn(async move {
                    let result = github::delete_repo(&name).await;
                    let _ = tx.send(TaskResult {
                        success: result.success,
                        message: if result.success {
                            format!("Deleted remote {}", name)
                        } else {
                            format!("Failed to delete {} (E: view errors)", name)
                        },
                        stderr: Some(result.stderr),
                        operation: op,
                        invalidates_github_cache: true, // Remote repo deleted from GitHub
                    }).await;
                });
                self.close_popup();
            }
        } else {
            self.close_popup();
        }
        self.pending_delete = None;
    }

    pub fn reorganize_to_ghq(&mut self) {
        let info = self.get_selected_repo().map(|r| {
            (
                r.name.clone(),
                r.local_path.clone(),
                r.expected_ghq_path(&self.local_root),
                r.follows_ghq(&self.local_root),
            )
        });

        if let Some((name, Some(current_path), Some(expected_path), Some(false))) = info {
            // Safety check: canonicalize both paths and compare to avoid copying directory to itself
            let src_canonical = Path::new(&current_path).canonicalize();
            let dst_canonical = Path::new(&expected_path).canonicalize();

            // If both paths canonicalize to the same location, repo is already in place
            if let (Ok(src), Ok(dst)) = (&src_canonical, &dst_canonical) {
                if src == dst {
                    self.set_status(format!("{} is already in ghq path", name));
                    return;
                }
            }

            // If destination already exists (but isn't the same as source), don't overwrite
            if dst_canonical.is_ok() {
                self.set_status(format!("Destination already exists for {}", name));
                return;
            }

            self.set_status(format!("Reorganizing {}...", name));
            let tx = self.task_tx.clone();
            let op = format!("reorganize {}", name);
            tokio::spawn(async move {
                let result = async {
                    let src = Path::new(&current_path);
                    let dst = Path::new(&expected_path);

                    // Create parent directories
                    if let Some(parent) = dst.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }

                    // Try simple rename first (works on same filesystem)
                    match tokio::fs::rename(src, dst).await {
                        Ok(()) => Ok(()),
                        Err(e) => {
                            // If rename fails (cross-device or target exists), try recursive copy
                            if e.kind() == std::io::ErrorKind::Other
                                || e.kind() == std::io::ErrorKind::AlreadyExists
                                || e.raw_os_error() == Some(18) // EXDEV - cross-device link
                                || e.raw_os_error() == Some(39) // ENOTEMPTY
                            {
                                // Recursive copy using system cp command for reliability
                                let status = tokio::process::Command::new("cp")
                                    .args(["-r", &current_path, &expected_path])
                                    .status()
                                    .await?;

                                if status.success() {
                                    // Remove original after successful copy
                                    tokio::fs::remove_dir_all(src).await?;
                                    Ok(())
                                } else {
                                    Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        "cp command failed",
                                    ))
                                }
                            } else {
                                Err(e)
                            }
                        }
                    }
                }.await;

                let _ = tx.send(TaskResult {
                    success: result.is_ok(),
                    message: if result.is_ok() {
                        format!("Moved {} to ghq path", name)
                    } else {
                        "Move failed (E: view errors)".to_string()
                    },
                    stderr: result.err().map(|e| e.to_string()),
                    operation: op,
                    invalidates_github_cache: false, // Local filesystem operation
                }).await;
            });
        }
    }

    pub fn toggle_private(&mut self) {
        let info = self.get_selected_repo().and_then(|r| {
            r.owner.clone().map(|o| (format!("{}/{}", o, r.name), r.is_private, r.is_archived))
        });
        if let Some((name_with_owner, is_private, is_archived)) = info {
            let new_visibility = if is_private { "public" } else { "private" };
            let status_msg = if is_archived {
                format!("Unarchiving, setting {} to {}, then re-archiving...", name_with_owner, new_visibility)
            } else {
                format!("Setting {} to {}...", name_with_owner, new_visibility)
            };
            self.set_status(status_msg);
            let tx = self.task_tx.clone();
            let name = name_with_owner.clone();
            let vis = new_visibility.to_string();
            let op = format!("set visibility {}", name);
            tokio::spawn(async move {
                // If archived, unarchive first
                if is_archived {
                    let unarchive_result = github::set_archived(&name, false).await;
                    if !unarchive_result.success {
                        let _ = tx.send(TaskResult {
                            success: false,
                            message: "Failed to unarchive before visibility change (E: view errors)".to_string(),
                            stderr: Some(unarchive_result.stderr),
                            operation: op,
                            invalidates_github_cache: true, // GitHub state may have changed
                        }).await;
                        return;
                    }
                }

                // Change visibility
                let result = github::set_visibility(&name, &vis).await;

                // If was archived, re-archive regardless of visibility result
                let rearchive_result = if is_archived {
                    Some(github::set_archived(&name, true).await)
                } else {
                    None
                };

                let (success, message, stderr) = match (result.success, rearchive_result) {
                    (true, None) => (true, format!("Set {} to {}", name, vis), None),
                    (true, Some(r)) if r.success => (true, format!("Set {} to {} (re-archived)", name, vis), None),
                    (true, Some(r)) => (false, format!("Set {} to {} but re-archive failed (E: view errors)", name, vis), Some(r.stderr)),
                    (false, Some(r)) if r.success => (false, "Visibility change failed, repo re-archived (E: view errors)".to_string(), Some(result.stderr)),
                    (false, Some(r)) => (false, "Visibility change failed, re-archive also failed (E: view errors)".to_string(), Some(format!("{}\n\nRe-archive error:\n{}", result.stderr, r.stderr))),
                    (false, None) => (false, "Visibility change failed (E: view errors)".to_string(), Some(result.stderr)),
                };

                let _ = tx.send(TaskResult {
                    success,
                    message,
                    stderr,
                    operation: op,
                    invalidates_github_cache: true, // GitHub visibility/archive state changed
                }).await;
            });
        }
    }

    pub fn toggle_archived(&mut self) {
        let info = self.get_selected_repo().and_then(|r| {
            if r.is_member {
                r.owner.clone().map(|o| (format!("{}/{}", o, r.name), r.is_archived))
            } else {
                None
            }
        });
        if let Some((name_with_owner, is_archived)) = info {
            let action = if is_archived { "Unarchiving" } else { "Archiving" };
            self.set_status(format!("{} {}...", action, name_with_owner));
            let tx = self.task_tx.clone();
            let name = name_with_owner.clone();
            let done = if is_archived { "Unarchived" } else { "Archived" };
            let op = format!("{} {}", action.to_lowercase(), name);
            tokio::spawn(async move {
                let result = github::set_archived(&name, !is_archived).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("{} {}", done, name)
                    } else {
                        format!("{} failed (E: view errors)", action)
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: true, // GitHub archive state changed
                }).await;
            });
        }
    }

    // Gist operations
    pub fn clone_gist(&mut self) {
        let info = self.get_selected_gist().and_then(|g| {
            if g.local_path.is_none() {
                Some(g.id.clone())
            } else {
                None
            }
        });
        if let Some(id) = info {
            let clone_path = format!("{}/gists/{}", self.local_root, id);
            let display_id = id[..8.min(id.len())].to_string();
            self.set_status(format!("Cloning gist {}...", display_id));
            let tx = self.task_tx.clone();
            let op = format!("clone gist {}", display_id);
            tokio::spawn(async move {
                let result = github::clone_gist(&id, &clone_path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Cloned gist {}", display_id)
                    } else {
                        "Clone gist failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Clone creates local copy, doesn't change GitHub
                }).await;
            });
        }
    }

    pub fn start_gist_delete_confirm(&mut self) {
        if self.get_selected_gist().is_some() {
            self.input_mode = InputMode::ConfirmDelete;
            self.pending_delete = Some(DeleteType::Gist);
            self.confirm_buffer.clear();
        }
    }

    pub fn delete_gist(&mut self) {
        if self.confirm_buffer.to_lowercase() == "y" || self.confirm_buffer.to_lowercase() == "yes" {
            let id = self.get_selected_gist().map(|g| g.id.clone());
            if let Some(id) = id {
                let display_id = id[..8.min(id.len())].to_string();
                self.set_status(format!("Deleting gist {}...", display_id));
                let tx = self.task_tx.clone();
                let op = format!("delete gist {}", display_id);
                let gist_id = id.clone();
                tokio::spawn(async move {
                    let result = github::delete_gist(&gist_id).await;
                    let _ = tx.send(TaskResult {
                        success: result.success,
                        message: if result.success {
                            format!("Deleted gist {}", display_id)
                        } else {
                            format!("Failed to delete gist {} (E: view errors)", display_id)
                        },
                        stderr: Some(result.stderr),
                        operation: op,
                        invalidates_github_cache: true, // Gist deleted from GitHub
                    }).await;
                });
                self.close_popup();
            }
        } else {
            self.close_popup();
        }
        self.pending_delete = None;
    }

    pub fn pull_gist(&mut self) {
        let info = self.get_selected_gist().and_then(|g| {
            g.local_path.clone().map(|p| (g.id.clone(), p))
        });
        if let Some((id, path)) = info {
            let display_id = id[..8.min(id.len())].to_string();
            self.set_status(format!("Pulling gist {}...", display_id));
            let tx = self.task_tx.clone();
            let op = format!("pull gist {}", display_id);
            tokio::spawn(async move {
                let result = git::pull(&path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Pulled gist {}", display_id)
                    } else {
                        "Pull gist failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    pub fn push_gist(&mut self) {
        let info = self.get_selected_gist().and_then(|g| {
            g.local_path.clone().map(|p| (g.id.clone(), p))
        });
        if let Some((id, path)) = info {
            let display_id = id[..8.min(id.len())].to_string();
            self.set_status(format!("Pushing gist {}...", display_id));
            let tx = self.task_tx.clone();
            let op = format!("push gist {}", display_id);
            tokio::spawn(async move {
                let result = git::push(&path).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Pushed gist {}", display_id)
                    } else {
                        "Push gist failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    pub fn sync_gist(&mut self) {
        let info = self.get_selected_gist().and_then(|g| {
            g.local_path.clone().map(|p| (g.id.clone(), p))
        });
        if let Some((id, path)) = info {
            let display_id = id[..8.min(id.len())].to_string();
            self.set_status(format!("Syncing gist {}...", display_id));
            let tx = self.task_tx.clone();
            let op = format!("sync gist {}", display_id);
            tokio::spawn(async move {
                let fetch_res = git::fetch(&path).await;
                let pull_res = git::pull(&path).await;
                let push_res = git::push(&path).await;
                let success = fetch_res.success && pull_res.success && push_res.success;
                let stderr = if !success {
                    let mut errs = Vec::new();
                    if !fetch_res.stderr.is_empty() { errs.push(fetch_res.stderr); }
                    if !pull_res.stderr.is_empty() { errs.push(pull_res.stderr); }
                    if !push_res.stderr.is_empty() { errs.push(push_res.stderr); }
                    Some(errs.join("\n"))
                } else {
                    None
                };
                let _ = tx.send(TaskResult {
                    success,
                    message: if success {
                        format!("Synced gist {}", display_id)
                    } else {
                        "Sync gist failed (E: view errors)".to_string()
                    },
                    stderr,
                    operation: op,
                    invalidates_github_cache: false, // Local git operation
                }).await;
            });
        }
    }

    /// Show upload form for a local-only repo
    pub fn show_upload_form(&mut self) {
        let info = self.get_selected_repo().and_then(|r| {
            if r.is_local_only() {
                r.local_path.clone().map(|p| (r.name.clone(), p))
            } else {
                None
            }
        });
        if let Some((name, path)) = info {
            // Fetch orgs in background and update form when ready
            let tx = self.task_tx.clone();
            tokio::spawn(async move {
                let orgs = github::get_user_orgs().await.unwrap_or_default();
                // Send orgs as a special message - we'll parse it later
                let _ = tx.send(TaskResult {
                    success: true,
                    message: format!("__ORGS__:{}", orgs.join(",")),
                    stderr: None,
                    operation: String::new(),
                    invalidates_github_cache: false, // Not a real operation, just data fetch
                }).await;
            });

            self.upload_form = Some(UploadFormState {
                name,
                description: String::new(),
                private: true,
                orgs: Vec::new(), // Will be populated when orgs arrive
                selected_org: 0,  // 0 = personal account
                active_field: UploadField::Name,
                local_path: path,
            });
            self.input_mode = InputMode::UploadForm;
            self.popup = Some(Popup::new(PopupType::Upload, Vec::new()));
        }
    }

    /// Submit the upload form
    pub fn submit_upload_form(&mut self) {
        if let Some(form) = self.upload_form.take() {
            let org = if form.selected_org == 0 {
                None
            } else {
                form.orgs.get(form.selected_org - 1).cloned()
            };

            let opts = github::CreateRepoOptions {
                name: form.name.clone(),
                path: form.local_path.clone(),
                description: if form.description.is_empty() { None } else { Some(form.description) },
                private: form.private,
                org,
            };

            self.set_status(format!("Creating GitHub repo {}...", opts.name));
            let tx = self.task_tx.clone();
            let name = opts.name.clone();
            let op = format!("create repo {}", name);
            tokio::spawn(async move {
                let result = github::create_repo(&opts).await;
                let _ = tx.send(TaskResult {
                    success: result.success,
                    message: if result.success {
                        format!("Created {}", name)
                    } else {
                        "Create repo failed (E: view errors)".to_string()
                    },
                    stderr: if result.success { None } else { Some(result.stderr) },
                    operation: op,
                    invalidates_github_cache: true, // New repo created on GitHub
                }).await;
            });

            self.close_popup();
        }
    }

    /// Cancel upload form
    pub fn cancel_upload_form(&mut self) {
        self.upload_form = None;
        self.close_popup();
    }

    /// Navigate to next field in upload form
    pub fn upload_form_next_field(&mut self) {
        if let Some(ref mut form) = self.upload_form {
            form.active_field = form.active_field.next();
        }
    }

    /// Navigate to previous field in upload form
    pub fn upload_form_prev_field(&mut self) {
        if let Some(ref mut form) = self.upload_form {
            form.active_field = form.active_field.prev();
        }
    }

    /// Toggle private field in upload form
    pub fn upload_form_toggle_private(&mut self) {
        if let Some(ref mut form) = self.upload_form {
            form.private = !form.private;
        }
    }

    /// Cycle org selection in upload form
    pub fn upload_form_next_org(&mut self) {
        if let Some(ref mut form) = self.upload_form {
            let max = form.orgs.len(); // 0 is personal, then orgs
            form.selected_org = (form.selected_org + 1) % (max + 1);
        }
    }

    pub fn upload_form_prev_org(&mut self) {
        if let Some(ref mut form) = self.upload_form {
            let max = form.orgs.len();
            if form.selected_org == 0 {
                form.selected_org = max;
            } else {
                form.selected_org -= 1;
            }
        }
    }

    pub fn handle_char(&mut self, c: char) {
        match self.input_mode {
            InputMode::ConfirmDelete => {
                self.confirm_buffer.push(c);
            }
            InputMode::UploadForm => {
                if let Some(ref mut form) = self.upload_form {
                    match form.active_field {
                        UploadField::Name => form.name.push(c),
                        UploadField::Description => form.description.push(c),
                        UploadField::Private => {
                            // Space or Enter toggles
                            if c == ' ' {
                                form.private = !form.private;
                            }
                        }
                        UploadField::Org => {
                            // Space cycles org
                            if c == ' ' {
                                self.upload_form_next_org();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_backspace(&mut self) {
        match self.input_mode {
            InputMode::ConfirmDelete => {
                self.confirm_buffer.pop();
            }
            InputMode::UploadForm => {
                if let Some(ref mut form) = self.upload_form {
                    match form.active_field {
                        UploadField::Name => { form.name.pop(); }
                        UploadField::Description => { form.description.pop(); }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    pub fn popup_next(&mut self) {
        if let Some(ref mut popup) = self.popup {
            if popup.popup_type == PopupType::Ignored {
                let max = popup.content.len().saturating_sub(1);
                popup.selected = (popup.selected + 1).min(max);
            }
        }
    }

    pub fn popup_prev(&mut self) {
        if let Some(ref mut popup) = self.popup {
            if popup.popup_type == PopupType::Ignored {
                popup.selected = popup.selected.saturating_sub(1).max(2); // Min 2 to skip header
            }
        }
    }
}

fn normalize_github_url(url: &str) -> String {
    url.trim()
        .trim_end_matches(".git")
        .replace("git@github.com:", "https://github.com/")
        .to_lowercase()
}

fn get_ghq_path(root: &str, url: &str) -> String {
    let normalized = normalize_github_url(url);
    let path = normalized
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    format!("{}/{}", root, path)
}

fn merge_repos(github_repos: Vec<github::GitHubRepoInfo>, local_repos: Vec<local::LocalRepo>) -> Vec<RepoRow> {
    let mut result: Vec<RepoRow> = Vec::new();
    let mut local_by_url: HashMap<String, local::LocalRepo> = HashMap::new();

    // Index local repos by normalized URL
    for repo in local_repos {
        if let Some(ref url) = repo.remote_url {
            let normalized = normalize_github_url(url);
            local_by_url.insert(normalized, repo);
        } else {
            // Local-only repo (no remote)
            result.push(RepoRow {
                id: repo.path.clone(),
                owner: None,
                name: repo.name.clone(),
                github_url: None,
                ssh_url: None,
                is_fork: false,
                fork_parent: None,
                is_private: false,
                is_archived: false,
                is_member: false,
                local_path: Some(repo.path),
                git_status: Some(repo.status),
                last_commit_time: repo.last_commit_time,
                is_subrepo: repo.is_subrepo,
                parent_repo: repo.parent_repo,
                fork_ahead: None,
                fork_behind: None,
                has_git: repo.has_git,
            });
        }
    }

    // Process GitHub repos, matching with local
    for gh_repo in github_repos {
        let normalized_url = normalize_github_url(&gh_repo.url);
        let local = local_by_url.remove(&normalized_url);

        // Use local commit time if available, otherwise use GitHub's pushed_at
        let last_commit_time = local
            .as_ref()
            .and_then(|l| l.last_commit_time)
            .or(gh_repo.pushed_at);

        result.push(RepoRow {
            id: normalized_url,
            owner: Some(gh_repo.owner.clone()),
            name: gh_repo.name.clone(),
            github_url: Some(gh_repo.url),
            ssh_url: Some(gh_repo.ssh_url),
            is_fork: gh_repo.is_fork,
            fork_parent: gh_repo.fork_parent,
            is_private: gh_repo.is_private,
            is_archived: gh_repo.is_archived,
            is_member: gh_repo.is_member,
            local_path: local.as_ref().map(|l| l.path.clone()),
            git_status: local.as_ref().map(|l| l.status.clone()),
            last_commit_time,
            is_subrepo: local.as_ref().map(|l| l.is_subrepo).unwrap_or(false),
            parent_repo: local.as_ref().and_then(|l| l.parent_repo.clone()),
            fork_ahead: gh_repo.fork_ahead,
            fork_behind: gh_repo.fork_behind,
            has_git: local.as_ref().map(|l| l.has_git).unwrap_or(true),
        });
    }

    // Add any remaining local repos that weren't matched (different remote host, etc.)
    for (_, repo) in local_by_url {
        result.push(RepoRow {
            id: repo.path.clone(),
            owner: repo.remote_owner,
            name: repo.name.clone(),
            github_url: repo.remote_url.clone(),
            ssh_url: repo.remote_url,
            is_fork: false,
            fork_parent: None,
            is_private: false,
            is_archived: false,
            is_member: false, // Not from our GitHub query
            local_path: Some(repo.path),
            git_status: Some(repo.status),
            last_commit_time: repo.last_commit_time,
            is_subrepo: repo.is_subrepo,
            parent_repo: repo.parent_repo,
            fork_ahead: None,
            fork_behind: None,
            has_git: repo.has_git,
        });
    }

    // Sort by owner (None last), then by name
    result.sort_by(|a, b| {
        match (&a.owner, &b.owner) {
            (Some(oa), Some(ob)) => {
                let owner_cmp = oa.to_lowercase().cmp(&ob.to_lowercase());
                if owner_cmp == std::cmp::Ordering::Equal {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                } else {
                    owner_cmp
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    result
}

// Help content lines - format: "KEY|DESCRIPTION|COLOR" where COLOR is optional
// Colors: cyan, magenta, yellow, green, red, blue
pub fn get_help_content(view_mode: &ViewMode) -> Vec<String> {
    match view_mode {
        ViewMode::Repos => vec![
            "HEADER|Navigation".to_string(),
            "↑/↓/j/k|Move up/down|".to_string(),
            "←/→|Change sort column|".to_string(),
            "v|Reverse sort direction|".to_string(),
            ", .|Select prev/next column|".to_string(),
            "< >|Move column left/right|".to_string(),
            "Tab|Switch to Gists view|cyan".to_string(),
            "Enter|Show details|".to_string(),
            "E|Show error log|yellow".to_string(),
            "y|Copy popup to clipboard|".to_string(),
            "".to_string(),
            "HEADER|Git Actions".to_string(),
            "g|Open lazygit|green".to_string(),
            "l|Pull (ff-only)|cyan".to_string(),
            "h|Push|magenta".to_string(),
            "s|Sync (pull+push)|".to_string(),
            "y|Quicksync (rebase+add+commit+push)|yellow".to_string(),
            "r|Refresh all|".to_string(),
            "".to_string(),
            "HEADER|Repository".to_string(),
            "n|Clone repo (remote-only)|cyan".to_string(),
            "u|Upload local repo to GitHub|magenta".to_string(),
            "o|Open in browser|".to_string(),
            "O|Open in file manager|".to_string(),
            "p|Toggle private/public|".to_string(),
            "P|Show/hide private repos|".to_string(),
            "a|Toggle archived status|".to_string(),
            "A|Show/hide archived repos|".to_string(),
            "d|Delete local copy|red".to_string(),
            "D|Delete remote repo|red".to_string(),
            "z|Reorganize to ghq path|".to_string(),
            "i|Init git (nogit) / Ignore repo|".to_string(),
            "I|Show ignored repos|".to_string(),
            "".to_string(),
            "HEADER|Type Icons".to_string(),
            "● src|Your original repository|green".to_string(),
            "◌ clone|Clone from other owner|cyan".to_string(),
            "⑂|Fork (shows upstream)|magenta".to_string(),
            "◌ local|Local only (no remote)|blue".to_string(),
            "⊂ sub|Subrepo (nested in another)|yellow".to_string(),
            "○ nogit|Folder without git repo|red".to_string(),
            "".to_string(),
            "HEADER|Status Icons".to_string(),
            "✓|Synced with remote|green".to_string(),
            "↑|Ahead (unpushed)|magenta".to_string(),
            "↓|Behind (can pull)|cyan".to_string(),
            "⇅|Diverged|red".to_string(),
            "*|Dirty (uncommitted)|yellow".to_string(),
            "?|No remote configured|blue".to_string(),
            "".to_string(),
            "|Press ? or Esc to close|".to_string(),
        ],
        ViewMode::Gists => vec![
            "HEADER|Navigation".to_string(),
            "↑/↓/j/k|Move up/down|".to_string(),
            "Tab|Switch to Repos view|cyan".to_string(),
            "Enter|Show details|".to_string(),
            "".to_string(),
            "HEADER|Git Actions".to_string(),
            "l|Pull (not when dirty)|cyan".to_string(),
            "h|Push (not when dirty)|magenta".to_string(),
            "s|Sync (not when dirty)|".to_string(),
            "r|Refresh all|".to_string(),
            "".to_string(),
            "HEADER|Gist Actions".to_string(),
            "n|Clone gist locally|cyan".to_string(),
            "d|Delete gist from GitHub|red".to_string(),
            "".to_string(),
            "|Press ? or Esc to close|".to_string(),
        ],
    }
}


// Sorting helpers
fn repo_type_sort_order(repo: &RepoRow, username: &Option<String>) -> u8 {
    // Subrepos are grouped separately at the end
    if repo.is_subrepo {
        4 // Subrepo
    } else if repo.is_fork {
        2 // Fork
    } else if repo.github_url.is_some() {
        if let (Some(ref u), Some(ref o)) = (username, &repo.owner) {
            if u.eq_ignore_ascii_case(o) {
                0 // Source (owned)
            } else {
                1 // Clone (not owned)
            }
        } else {
            1 // Clone
        }
    } else {
        3 // Local only
    }
}

fn status_sort_order(repo: &RepoRow) -> u8 {
    match &repo.git_status {
        Some(status) => {
            if status.is_dirty() {
                0 // Dirty first
            } else if status.ahead > 0 && status.behind > 0 {
                1 // Diverged
            } else if status.ahead > 0 {
                2 // Ahead
            } else if status.behind > 0 {
                3 // Behind
            } else if !status.has_remote {
                5 // No remote
            } else {
                4 // Synced
            }
        }
        None => 6, // No local
    }
}
