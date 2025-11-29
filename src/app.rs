use crate::git::RepoStatus;
use crate::{git, github, local};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Repos,
    Gists,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Commit,
    ConfirmDelete,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PopupType {
    Help,
    DirtyFiles,
    Diff,
    Details,
    Ignored,
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

#[derive(Debug, Clone, PartialEq)]
pub enum RepoType {
    Source,    // Original repo (not a fork)
    Fork,      // Forked from another repo
    Clone,     // Local clone without GitHub association
}

#[derive(Debug, Clone)]
pub struct RepoRow {
    pub id: String,
    pub owner: Option<String>,
    pub name: String,
    pub github_url: Option<String>,
    pub ssh_url: Option<String>,
    pub is_fork: bool,
    pub fork_parent: Option<String>,
    pub is_private: bool,
    pub local_path: Option<String>,
    pub git_status: Option<RepoStatus>,
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

    pub fn repo_type(&self) -> RepoType {
        if self.is_fork {
            RepoType::Fork
        } else if self.github_url.is_some() {
            RepoType::Source
        } else {
            RepoType::Clone
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
    pub created_at: Option<String>,
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

// Running operation for async task tracking
#[derive(Debug, Clone)]
pub struct RunningOp {
    pub description: String,
    pub output: Vec<String>,
    pub completed: bool,
    pub success: Option<bool>,
}

pub struct App {
    pub local_root: String,
    pub view_mode: ViewMode,
    pub github_username: Option<String>,

    // Data
    pub repos: Vec<RepoRow>,
    pub gists: Vec<GistRow>,
    pub ignored_repos: HashSet<String>,

    // Selection
    pub selected: usize,
    pub scroll_offset: usize,

    // UI state
    pub status_message: Option<String>,
    pub input_mode: InputMode,
    pub popup: Option<Popup>,
    pub input_buffer: String,
    pub confirm_buffer: String,

    // Running operations
    pub running_ops: Vec<RunningOp>,
}

impl App {
    pub async fn new(local_root: String) -> Result<Self> {
        // Fetch github username
        let github_username = github::get_current_user().await.ok();

        let mut app = Self {
            local_root,
            view_mode: ViewMode::Repos,
            github_username,
            repos: Vec::new(),
            gists: Vec::new(),
            ignored_repos: HashSet::new(),
            selected: 0,
            scroll_offset: 0,
            status_message: Some("Loading...".to_string()),
            input_mode: InputMode::Normal,
            popup: None,
            input_buffer: String::new(),
            confirm_buffer: String::new(),
            running_ops: Vec::new(),
        };

        app.refresh().await?;
        app.status_message = None;

        Ok(app)
    }

    // Check if current user can modify repo visibility
    pub fn can_change_visibility(&self, repo: &RepoRow) -> bool {
        if let (Some(ref username), Some(ref owner)) = (&self.github_username, &repo.owner) {
            username.eq_ignore_ascii_case(owner)
        } else {
            false
        }
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.status_message = Some("Refreshing...".to_string());

        // Fetch GitHub repos via GraphQL
        let github_repos = github::fetch_all_repos_graphql().await.unwrap_or_default();

        // Discover local repos
        let local_repos = local::discover_repos(&self.local_root).await?;

        // Merge into unified list
        self.repos = merge_repos(github_repos, local_repos);

        // Fetch gists
        self.gists = github::fetch_gists_as_rows(&self.local_root).await.unwrap_or_default();

        // Reset selection if out of bounds
        let max = self.visible_list_len().saturating_sub(1);
        if self.selected > max {
            self.selected = max;
        }

        self.status_message = None;
        Ok(())
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Repos => ViewMode::Gists,
            ViewMode::Gists => ViewMode::Repos,
        };
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn visible_repos(&self) -> Vec<&RepoRow> {
        self.repos
            .iter()
            .filter(|r| !self.ignored_repos.contains(&r.id))
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
            if self.ignored_repos.contains(&id) {
                self.ignored_repos.remove(&id);
            } else {
                self.ignored_repos.insert(id);
                // Adjust selection if needed
                let max = self.visible_list_len().saturating_sub(1);
                if self.selected > max {
                    self.selected = max;
                }
            }
        }
    }

    // Show ignored repos popup
    pub fn show_ignored_popup(&mut self) {
        if self.ignored_repos.is_empty() {
            self.popup = Some(Popup::new(PopupType::Ignored, vec!["No ignored repositories.".to_string()]));
        } else {
            let mut content: Vec<String> = self.ignored_repos.iter().cloned().collect();
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
                let ignored_list: Vec<String> = self.ignored_repos.iter().cloned().collect();
                if let Some(id) = ignored_list.get(idx) {
                    self.ignored_repos.remove(id);
                }
                // Refresh popup
                self.show_ignored_popup();
            }
        }
    }

    // Git operations for selected repo
    pub async fn pull_selected(&mut self) -> Result<()> {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.status_message = Some(format!("Pulling {}...", name));
            git::pull(&path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn push_selected(&mut self) -> Result<()> {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.status_message = Some(format!("Pushing {}...", name));
            git::push(&path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn sync_selected(&mut self) -> Result<()> {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            self.status_message = Some(format!("Syncing {}...", name));
            git::fetch(&path).await?;
            git::pull(&path).await?;
            git::push(&path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn clone_selected(&mut self) -> Result<()> {
        let info = self.get_selected_repo().and_then(|r| {
            if r.is_remote_only() {
                r.ssh_url.clone().map(|url| (r.name.clone(), url))
            } else {
                None
            }
        });
        if let Some((name, url)) = info {
            let clone_path = get_ghq_path(&self.local_root, &url);
            self.status_message = Some(format!("Cloning {}...", name));
            git::clone(&url, &clone_path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn show_diff(&mut self) -> Result<()> {
        let path = self.get_selected_repo().and_then(|r| r.local_path.clone());
        if let Some(path) = path {
            let diff = git::get_diff(&path).await?;
            let lines: Vec<String> = diff.lines().map(|s| s.to_string()).collect();
            if !lines.is_empty() {
                self.popup = Some(Popup::new(PopupType::Diff, lines));
            } else {
                self.status_message = Some("No changes to show".to_string());
            }
        }
        Ok(())
    }

    pub fn start_commit(&mut self) {
        let is_dirty = self.get_selected_repo()
            .and_then(|r| r.git_status.as_ref())
            .map(|s| s.is_dirty())
            .unwrap_or(false);
        if is_dirty {
            self.input_mode = InputMode::Commit;
            self.input_buffer = "fixup: from ghall".to_string();
        }
    }

    pub async fn commit_and_push(&mut self) -> Result<()> {
        let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
        if let Some((name, Some(path))) = info {
            let message = if self.input_buffer.is_empty() {
                "fixup: from ghall".to_string()
            } else {
                self.input_buffer.clone()
            };

            self.status_message = Some(format!("Committing {}...", name));
            git::add_all(&path).await?;
            git::commit(&path, &message).await?;
            git::push(&path).await?;

            self.close_popup();
            self.refresh().await?;
        }
        Ok(())
    }

    pub fn start_delete_confirm(&mut self) {
        let has_local = self.get_selected_repo()
            .map(|r| r.has_local())
            .unwrap_or(false);
        if has_local {
            self.input_mode = InputMode::ConfirmDelete;
            self.confirm_buffer.clear();
        }
    }

    pub async fn delete_local_repo(&mut self) -> Result<()> {
        if self.confirm_buffer.to_lowercase() == "y" || self.confirm_buffer.to_lowercase() == "yes" {
            let info = self.get_selected_repo().map(|r| (r.name.clone(), r.local_path.clone()));
            if let Some((name, Some(path))) = info {
                self.status_message = Some(format!("Deleting {}...", name));
                tokio::fs::remove_dir_all(&path).await?;
                self.close_popup();
                self.refresh().await?;
            }
        } else {
            self.close_popup();
        }
        Ok(())
    }

    pub async fn create_github_repo(&mut self) -> Result<()> {
        let info = self.get_selected_repo().and_then(|r| {
            if r.is_local_only() {
                r.local_path.clone().map(|p| (r.name.clone(), p))
            } else {
                None
            }
        });
        if let Some((name, path)) = info {
            self.status_message = Some(format!("Creating GitHub repo for {}...", name));
            github::create_repo(&name, &path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn toggle_private(&mut self) -> Result<()> {
        let info = self.get_selected_repo().and_then(|r| {
            r.owner.clone().map(|o| (format!("{}/{}", o, r.name), r.is_private))
        });
        if let Some((name_with_owner, is_private)) = info {
            let new_visibility = if is_private { "public" } else { "private" };
            self.status_message = Some(format!("Setting {} to {}...", name_with_owner, new_visibility));
            github::set_visibility(&name_with_owner, new_visibility).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    // Gist operations
    pub async fn clone_gist(&mut self) -> Result<()> {
        let info = self.get_selected_gist().and_then(|g| {
            if g.local_path.is_none() {
                Some(g.id.clone())
            } else {
                None
            }
        });
        if let Some(id) = info {
            let clone_path = format!("{}/gists/{}", self.local_root, id);
            let display_id = &id[..8.min(id.len())];
            self.status_message = Some(format!("Cloning gist {}...", display_id));
            github::clone_gist(&id, &clone_path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub fn start_gist_delete_confirm(&mut self) {
        if self.get_selected_gist().is_some() {
            self.input_mode = InputMode::ConfirmDelete;
            self.confirm_buffer.clear();
        }
    }

    pub async fn delete_gist(&mut self) -> Result<()> {
        if self.confirm_buffer.to_lowercase() == "y" || self.confirm_buffer.to_lowercase() == "yes" {
            let id = self.get_selected_gist().map(|g| g.id.clone());
            if let Some(id) = id {
                self.status_message = Some("Deleting gist...".to_string());
                github::delete_gist(&id).await?;
                self.close_popup();
                self.refresh().await?;
            }
        } else {
            self.close_popup();
        }
        Ok(())
    }

    pub async fn pull_gist(&mut self) -> Result<()> {
        let info = self.get_selected_gist().and_then(|g| {
            g.local_path.clone().map(|p| (g.id.clone(), p))
        });
        if let Some((id, path)) = info {
            let display_id = &id[..8.min(id.len())];
            self.status_message = Some(format!("Pulling gist {}...", display_id));
            git::pull(&path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn push_gist(&mut self) -> Result<()> {
        let info = self.get_selected_gist().and_then(|g| {
            g.local_path.clone().map(|p| (g.id.clone(), p))
        });
        if let Some((id, path)) = info {
            let display_id = &id[..8.min(id.len())];
            self.status_message = Some(format!("Pushing gist {}...", display_id));
            git::push(&path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn sync_gist(&mut self) -> Result<()> {
        let info = self.get_selected_gist().and_then(|g| {
            g.local_path.clone().map(|p| (g.id.clone(), p))
        });
        if let Some((id, path)) = info {
            let display_id = &id[..8.min(id.len())];
            self.status_message = Some(format!("Syncing gist {}...", display_id));
            git::fetch(&path).await?;
            git::pull(&path).await?;
            git::push(&path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub async fn show_gist_diff(&mut self) -> Result<()> {
        let path = self.get_selected_gist().and_then(|g| g.local_path.clone());
        if let Some(path) = path {
            let diff = git::get_diff(&path).await?;
            let lines: Vec<String> = diff.lines().map(|s| s.to_string()).collect();
            if !lines.is_empty() {
                self.popup = Some(Popup::new(PopupType::Diff, lines));
            } else {
                self.status_message = Some("No changes to show".to_string());
            }
        }
        Ok(())
    }

    pub async fn upload_local_repo(&mut self) -> Result<()> {
        let info = self.get_selected_repo().and_then(|r| {
            if r.is_local_only() {
                r.local_path.clone().map(|p| (r.name.clone(), p))
            } else {
                None
            }
        });
        if let Some((name, path)) = info {
            self.status_message = Some(format!("Creating GitHub repo {}...", name));
            github::create_repo(&name, &path).await?;
            self.refresh().await?;
        }
        Ok(())
    }

    pub fn handle_char(&mut self, c: char) {
        match self.input_mode {
            InputMode::Commit => {
                self.input_buffer.push(c);
            }
            InputMode::ConfirmDelete => {
                self.confirm_buffer.push(c);
            }
            _ => {}
        }
    }

    pub fn handle_backspace(&mut self) {
        match self.input_mode {
            InputMode::Commit => {
                self.input_buffer.pop();
            }
            InputMode::ConfirmDelete => {
                self.confirm_buffer.pop();
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
                local_path: Some(repo.path),
                git_status: Some(repo.status),
            });
        }
    }

    // Process GitHub repos, matching with local
    for gh_repo in github_repos {
        let normalized_url = normalize_github_url(&gh_repo.url);
        let local = local_by_url.remove(&normalized_url);

        result.push(RepoRow {
            id: normalized_url,
            owner: Some(gh_repo.owner.clone()),
            name: gh_repo.name.clone(),
            github_url: Some(gh_repo.url),
            ssh_url: Some(gh_repo.ssh_url),
            is_fork: gh_repo.is_fork,
            fork_parent: gh_repo.fork_parent,
            is_private: gh_repo.is_private,
            local_path: local.as_ref().map(|l| l.path.clone()),
            git_status: local.map(|l| l.status),
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
            local_path: Some(repo.path),
            git_status: Some(repo.status),
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
            "j/↓|Move down|".to_string(),
            "k/↑|Move up|".to_string(),
            "g|Switch to Gists view|cyan".to_string(),
            "Enter|Show details|".to_string(),
            "".to_string(),
            "HEADER|Git Actions".to_string(),
            "l|Pull (fast-forward)|cyan".to_string(),
            "h|Push to remote|magenta".to_string(),
            "s|Sync (fetch+pull+push)|green".to_string(),
            "f|Show diff|yellow".to_string(),
            "r|Refresh all|".to_string(),
            "".to_string(),
            "HEADER|Repository".to_string(),
            "n|Clone repo (remote-only)|cyan".to_string(),
            "u|Upload local repo to GitHub|magenta".to_string(),
            "p|Toggle private/public|".to_string(),
            "d|Delete local copy|red".to_string(),
            "i|Ignore/hide repo|".to_string(),
            "I|Show ignored repos|".to_string(),
            "".to_string(),
            "HEADER|Type Icons".to_string(),
            "● src|Original repository|green".to_string(),
            "⑂|Fork (shows upstream)|magenta".to_string(),
            "◌ local|Local clone only|blue".to_string(),
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
            "j/↓|Move down|".to_string(),
            "k/↑|Move up|".to_string(),
            "g|Switch to Repos view|cyan".to_string(),
            "Enter|Show details|".to_string(),
            "".to_string(),
            "HEADER|Git Actions".to_string(),
            "l|Pull (fast-forward)|cyan".to_string(),
            "h|Push to remote|magenta".to_string(),
            "s|Sync (fetch+pull+push)|green".to_string(),
            "f|Show diff|yellow".to_string(),
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
