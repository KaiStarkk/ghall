use crate::app::{App, DeleteType, GistRow, InputMode, PopupType, RepoRow, SortColumn, UploadField, ViewMode};
use crate::config::Column;
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table,
    },
    Frame,
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title bar with mode tabs
            Constraint::Min(0),    // Main content (table)
            Constraint::Length(2), // Status bar (2 lines for all hotkeys)
        ])
        .split(f.area());

    // Title bar with view mode tabs
    draw_title_bar(f, chunks[0], app);

    // Main content - table (store area for mouse detection)
    let table_area = chunks[1];
    app.table_area = Some((table_area.y, table_area.height));
    match app.view_mode {
        ViewMode::Repos => draw_repos_table(f, table_area, app),
        ViewMode::Gists => draw_gists_table(f, table_area, app),
    }

    // Status bar (2 lines)
    draw_status_bar(f, chunks[2], app);

    // Draw popups/input modes
    match app.input_mode {
        InputMode::ConfirmDelete => draw_confirm_delete_popup(f, app),
        InputMode::UploadForm => draw_upload_form_popup(f, app),
        InputMode::Normal => {
            if let Some(ref popup) = app.popup {
                draw_popup(f, popup);
            }
        }
    }
}

fn draw_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let (repos_style, gists_style) = match app.view_mode {
        ViewMode::Repos => (
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        ),
        ViewMode::Gists => (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
    };

    let title = Line::from(vec![
        Span::styled(" ghall ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("│ "),
        Span::styled("Repos", repos_style),
        Span::raw("  "),
        Span::styled("Gists", gists_style),
        Span::styled("  (Tab to switch)", Style::default().fg(Color::DarkGray)),
    ]);

    f.render_widget(Paragraph::new(title), area);
}

fn draw_repos_table(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let repos = app.visible_repos();

    if repos.is_empty() {
        let empty = Paragraph::new("No repositories found. Press 'r' to refresh.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, inner);
        return;
    }

    let columns = app.visible_columns();
    let selected_col = app.selected_column_index();

    // Build widths dynamically based on visible columns
    let widths: Vec<Constraint> = columns.iter().map(|col| {
        let w = col.width();
        if w == 0 {
            Constraint::Min(20) // Path column takes remainder
        } else {
            Constraint::Length(w)
        }
    }).collect();

    // Build header cells dynamically
    let header_cells: Vec<Cell> = columns.iter().enumerate().map(|(idx, col)| {
        let sort_col = SortColumn::from_column(*col);
        let name = format_header(col.name(), sort_col, app);
        let style = if idx == selected_col {
            Style::default().add_modifier(Modifier::BOLD).add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        Cell::from(name).style(style)
    }).collect();

    let header = Row::new(header_cells)
        .style(Style::default().fg(Color::Cyan))
        .height(1);

    // Rows - build cells dynamically based on visible columns
    let rows: Vec<Row> = repos
        .iter()
        .enumerate()
        .map(|(idx, repo)| {
            let is_selected = idx == app.selected;
            let row_style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let cells: Vec<Cell> = columns.iter().map(|col| {
                match col {
                    Column::Origin => Cell::from(format_origin(repo)),
                    Column::Repository => Cell::from(format_repo_name(repo)),
                    Column::Type => Cell::from(format_type(repo)),
                    Column::Updated => Cell::from(format_updated(repo)),
                    Column::Archived => Cell::from(format_archived(repo)),
                    Column::Private => Cell::from(format_private(repo)),
                    Column::Ghq => Cell::from(format_ghq(repo, app)),
                    Column::Status => Cell::from(format_status(repo)),
                    Column::Dirty => Cell::from(format_dirty(repo)),
                    Column::Path => Cell::from(format_path(repo)),
                }
            }).collect();

            Row::new(cells).style(row_style)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(Color::DarkGray));

    f.render_widget(table, inner);
}

fn draw_gists_table(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.gists.is_empty() {
        let empty = Paragraph::new("No gists found. Press 'r' to refresh.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, inner);
        return;
    }

    // Column widths - pack left
    let widths = [
        Constraint::Min(30),     // Description
        Constraint::Length(6),   // Files
        Constraint::Length(7),   // Public
        Constraint::Length(3),   // Dirty
        Constraint::Length(10),  // Status
        Constraint::Length(25),  // Path
    ];

    // Header
    let header = Row::new(vec![
        Cell::from("Description").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Files").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Public").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("").style(Style::default().add_modifier(Modifier::BOLD)), // Dirty
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Path").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .style(Style::default().fg(Color::Cyan))
    .height(1);

    // Rows
    let rows: Vec<Row> = app
        .gists
        .iter()
        .enumerate()
        .map(|(idx, gist)| {
            let is_selected = idx == app.selected;
            let row_style = if is_selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(format_gist_description(gist)),
                Cell::from(format!("{}", gist.file_names.len())),
                Cell::from(if gist.is_public { "✓" } else { "" }),
                Cell::from(format_gist_dirty(gist)),
                Cell::from(format_gist_status(gist)),
                Cell::from(format_gist_local(gist)),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().bg(Color::DarkGray));

    f.render_widget(table, inner);
}

// Formatting helpers for repos table
fn format_origin(repo: &RepoRow) -> Span<'static> {
    match &repo.owner {
        Some(owner) => {
            let display = truncate(owner, 13);
            // Grey if local exists (we have it), solid if remote-only (we don't have it)
            let style = if repo.has_local() {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            Span::styled(display, style)
        }
        None => Span::styled("(local)", Style::default().fg(Color::Blue)),
    }
}

fn format_repo_name(repo: &RepoRow) -> Span<'static> {
    let name = truncate(&repo.name, 19);
    let style = if repo.is_local_only() {
        Style::default().fg(Color::Blue)
    } else if repo.has_local() {
        // Has both local and remote - grey (we have it)
        Style::default().fg(Color::DarkGray)
    } else {
        // Remote only - normal (we don't have it)
        Style::default()
    };
    Span::styled(name, style)
}

fn format_type(repo: &RepoRow) -> Line<'static> {
    // Check if this is a fork
    if repo.is_fork {
        // Fork symbol in purple + upstream owner
        let mut spans = vec![Span::styled("⑂ ", Style::default().fg(Color::Magenta))];
        if let Some(parent_owner) = repo.fork_owner() {
            spans.push(Span::styled(
                truncate(parent_owner, 14),
                Style::default().fg(Color::Magenta),
            ));
        }
        return Line::from(spans);
    }

    if repo.github_url.is_some() && repo.is_member {
        // Source indicator (owned by user or their org)
        Line::from(Span::styled("● src", Style::default().fg(Color::Green)))
    } else if repo.github_url.is_some() {
        // Clone from another owner (not a fork, not owned by user)
        Line::from(Span::styled("◌ clone", Style::default().fg(Color::Cyan)))
    } else {
        // Local only
        Line::from(Span::styled("◌ local", Style::default().fg(Color::Blue)))
    }
}

fn format_private(repo: &RepoRow) -> Span<'static> {
    if repo.is_private {
        Span::styled("🔒", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    }
}

fn format_archived(repo: &RepoRow) -> Span<'static> {
    if repo.is_archived {
        Span::styled("📦", Style::default().fg(Color::DarkGray))
    } else {
        Span::raw("")
    }
}

fn format_dirty(repo: &RepoRow) -> Span<'static> {
    if let Some(ref status) = repo.git_status {
        if status.is_dirty() {
            Span::styled("*", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("")
        }
    } else {
        Span::raw("")
    }
}

fn format_ghq(repo: &RepoRow, app: &App) -> Span<'static> {
    match repo.follows_ghq(&app.local_root) {
        Some(true) => Span::styled("✓", Style::default().fg(Color::Green)),
        Some(false) => Span::styled("✗", Style::default().fg(Color::Red)),
        None => Span::raw(""), // No local or no GitHub info
    }
}

fn format_updated(repo: &RepoRow) -> Span<'static> {
    match repo.last_commit_time {
        Some(timestamp) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let diff_secs = now - timestamp;

            let text = if diff_secs < 60 {
                "just now".to_string()
            } else if diff_secs < 3600 {
                format!("{}m ago", diff_secs / 60)
            } else if diff_secs < 86400 {
                format!("{}h ago", diff_secs / 3600)
            } else if diff_secs < 604800 {
                format!("{}d ago", diff_secs / 86400)
            } else if diff_secs < 2592000 {
                format!("{}w ago", diff_secs / 604800)
            } else if diff_secs < 31536000 {
                format!("{}mo ago", diff_secs / 2592000)
            } else {
                format!("{}y ago", diff_secs / 31536000)
            };

            let color = if diff_secs < 86400 {
                Color::Green  // < 1 day
            } else if diff_secs < 604800 {
                Color::Yellow // < 1 week
            } else {
                Color::DarkGray // older
            };

            Span::styled(text, Style::default().fg(color))
        }
        None => Span::styled("—", Style::default().fg(Color::DarkGray)),
    }
}

fn format_status(repo: &RepoRow) -> Span<'static> {
    match &repo.git_status {
        Some(status) => {
            if !status.has_remote {
                return Span::styled("?", Style::default().fg(Color::Blue));
            }

            // Dirty takes precedence over ahead/behind
            if status.is_dirty() {
                return Span::styled("~", Style::default().fg(Color::Yellow));
            }

            let text = if status.ahead > 0 && status.behind > 0 {
                format!("⇅ +{}/-{}", status.ahead, status.behind)
            } else if status.ahead > 0 {
                format!("↑ +{}", status.ahead)
            } else if status.behind > 0 {
                format!("↓ -{}", status.behind)
            } else {
                "✓".to_string()
            };

            let color = if status.ahead > 0 && status.behind > 0 {
                Color::Red
            } else if status.ahead > 0 {
                Color::Magenta
            } else if status.behind > 0 {
                Color::Cyan
            } else {
                Color::Green
            };

            Span::styled(text, Style::default().fg(color))
        }
        None => Span::raw(""),
    }
}

fn format_path(repo: &RepoRow) -> Span<'static> {
    match &repo.local_path {
        Some(path) => {
            let short_path = shorten_path(path);
            let truncated = truncate(&short_path, 35);
            Span::styled(truncated, Style::default())
        }
        None => Span::styled("—", Style::default().fg(Color::DarkGray)),
    }
}

// Formatting helpers for gists table
fn format_gist_description(gist: &GistRow) -> Span<'static> {
    let desc = if gist.description.is_empty() {
        gist.file_names.first().cloned().unwrap_or_else(|| "Untitled".to_string())
    } else {
        gist.description.clone()
    };
    // Grey if local exists, solid if remote-only
    let style = if gist.has_local() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };
    Span::styled(truncate(&desc, 40), style)
}

fn format_gist_dirty(gist: &GistRow) -> Span<'static> {
    if gist.is_dirty() {
        Span::styled("*", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("")
    }
}

fn format_gist_status(gist: &GistRow) -> Span<'static> {
    match &gist.git_status {
        Some(status) => {
            if !status.has_remote {
                return Span::styled("?", Style::default().fg(Color::Blue));
            }

            // Dirty takes precedence over ahead/behind
            if status.is_dirty() {
                return Span::styled("~", Style::default().fg(Color::Yellow));
            }

            let text = if status.ahead > 0 && status.behind > 0 {
                format!("⇅ +{}/-{}", status.ahead, status.behind)
            } else if status.ahead > 0 {
                format!("↑ +{}", status.ahead)
            } else if status.behind > 0 {
                format!("↓ -{}", status.behind)
            } else {
                "✓".to_string()
            };

            let color = if status.ahead > 0 && status.behind > 0 {
                Color::Red
            } else if status.ahead > 0 {
                Color::Magenta
            } else if status.behind > 0 {
                Color::Cyan
            } else {
                Color::Green
            };

            Span::styled(text, Style::default().fg(color))
        }
        None => Span::raw(""),
    }
}

fn format_gist_local(gist: &GistRow) -> Span<'static> {
    match &gist.local_path {
        Some(path) => {
            let short = shorten_path(path);
            Span::styled(truncate(&short, 23), Style::default())
        }
        None => Span::styled("—", Style::default().fg(Color::DarkGray)),
    }
}

// Format column header with sort indicator
fn format_header(name: &str, column: SortColumn, app: &App) -> String {
    if app.sort_column == column {
        let arrow = if app.sort_ascending { "▲" } else { "▼" };
        format!("[{} {}]", name, arrow)
    } else {
        name.to_string()
    }
}

// Utility functions
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

fn shorten_path(path: &str) -> String {
    // Replace home directory with ~
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    }
}

/// Draw the status bar with all hotkeys (enabled ones normal, disabled ones grey)
fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    // If there's a status message, show it on first line
    if let Some(ref msg) = app.status_message {
        let status_line = Line::from(vec![
            Span::styled(format!("{} ", app.spinner_char()), Style::default().fg(Color::Cyan)),
            Span::styled(msg.clone(), Style::default().fg(Color::Yellow)),
        ]);
        f.render_widget(Paragraph::new(status_line), area);
        return;
    }

    // Popup mode - show popup-specific help
    if let Some(ref popup) = app.popup {
        let help = match popup.popup_type {
            PopupType::Details => "Enter/Esc: close",
            PopupType::Ignored => "j/k/↑/↓: select │ Enter: unhide │ Esc: close",
            _ => "j/k/↑/↓: scroll │ y: copy │ Esc: close",
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(help, Style::default().fg(Color::Gray)))),
            area,
        );
        return;
    }

    // Build hotkey lines based on current selection
    let (line1, line2) = match app.view_mode {
        ViewMode::Repos => build_repos_hotkeys(app),
        ViewMode::Gists => build_gists_hotkeys(app),
    };

    f.render_widget(Paragraph::new(vec![line1, line2]), area);
}

/// Helper to create a hotkey span (enabled or disabled)
fn hotkey(key: &str, desc: &str, enabled: bool) -> Vec<Span<'static>> {
    let style = if enabled {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    vec![
        Span::styled(key.to_string(), style),
        Span::styled(format!(":{} ", desc), style),
    ]
}

/// Build repos mode hotkey lines
fn build_repos_hotkeys(app: &App) -> (Line<'static>, Line<'static>) {
    let repo = app.get_selected_repo();
    let has_local = repo.map(|r| r.has_local()).unwrap_or(false);
    let is_remote_only = repo.map(|r| r.is_remote_only()).unwrap_or(false);
    let is_local_only = repo.map(|r| r.is_local_only()).unwrap_or(false);
    let is_dirty = repo.and_then(|r| r.git_status.as_ref()).map(|s| s.is_dirty()).unwrap_or(false);
    let can_change = repo.map(|r| app.can_change_visibility(r)).unwrap_or(false);
    let has_github = repo.map(|r| r.github_url.is_some()).unwrap_or(false);
    let needs_ghq = repo.map(|r| r.follows_ghq(&app.local_root) == Some(false)).unwrap_or(false);

    // Error indicator
    let mut spans1: Vec<Span> = if app.error_count() > 0 {
        vec![Span::styled(format!("[{}err] ", app.error_count()), Style::default().fg(Color::Red))]
    } else {
        vec![]
    };

    // Line 1: Navigation + Git operations
    spans1.extend(hotkey("↑↓", "nav", true));
    spans1.extend(hotkey("←→", "sort", true));
    spans1.extend(hotkey("v", "rev", true));
    spans1.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
    spans1.extend(hotkey("n", "clone", is_remote_only));
    spans1.extend(hotkey("l", "pull", has_local && !is_dirty));
    spans1.extend(hotkey("h", "push", has_local && !is_dirty));
    spans1.extend(hotkey("s", "sync", has_local && !is_dirty));
    spans1.extend(hotkey("y", "qsync", has_local));
    spans1.extend(hotkey("g", "git", has_local));

    // Line 2: Repo actions + filters
    let mut spans2: Vec<Span> = vec![];
    spans2.extend(hotkey("p", "priv", can_change));
    spans2.extend(hotkey("a", "arch", can_change));
    spans2.extend(hotkey("o", "web", has_github));
    spans2.extend(hotkey("O", "files", has_local));
    spans2.extend(hotkey("u", "upload", is_local_only));
    spans2.extend(hotkey("z", "ghq", needs_ghq));
    spans2.extend(hotkey("d", "del", has_local));
    spans2.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
    spans2.extend(hotkey("A", "arch", true));
    spans2.extend(hotkey("P", "priv", true));
    spans2.extend(hotkey("i", "hide", true));
    spans2.extend(hotkey("r", "ref", true));
    spans2.extend(hotkey("?", "help", true));

    (Line::from(spans1), Line::from(spans2))
}

/// Build gists mode hotkey lines
fn build_gists_hotkeys(app: &App) -> (Line<'static>, Line<'static>) {
    let gist = app.get_selected_gist();
    let has_local = gist.map(|g| g.has_local()).unwrap_or(false);
    let is_remote_only = gist.map(|g| g.local_path.is_none()).unwrap_or(false);
    let is_dirty = gist.map(|g| g.is_dirty()).unwrap_or(false);

    let mut spans1: Vec<Span> = vec![];
    spans1.extend(hotkey("↑↓", "nav", true));
    spans1.extend(hotkey("Enter", "details", true));
    spans1.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
    spans1.extend(hotkey("n", "clone", is_remote_only));
    spans1.extend(hotkey("l", "pull", has_local && !is_dirty));
    spans1.extend(hotkey("h", "push", has_local && !is_dirty));
    spans1.extend(hotkey("s", "sync", has_local && !is_dirty));

    let mut spans2: Vec<Span> = vec![];
    spans2.extend(hotkey("d", "delete", true));
    spans2.extend(hotkey("r", "refresh", true));
    spans2.extend(hotkey("Tab", "repos", true));
    spans2.extend(hotkey("?", "help", true));

    (Line::from(spans1), Line::from(spans2))
}

fn draw_popup(f: &mut Frame, popup: &crate::app::Popup) {
    let (width, height) = match popup.popup_type {
        PopupType::Help => (55, 70),
        PopupType::Details => (60, 50),
        PopupType::Ignored => (60, 50),
        PopupType::Errors => (70, 60),
        PopupType::Upload => return, // Upload form is drawn by draw_upload_form_popup
    };

    let area = centered_rect(width, height, f.area());
    f.render_widget(Clear, area);

    let title = match popup.popup_type {
        PopupType::Help => " Help ",
        PopupType::Details => " Details ",
        PopupType::Ignored => " Ignored Repos ",
        PopupType::Errors => " Error Log ",
        PopupType::Upload => " Upload ",
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    // Calculate visible content with scroll
    let visible_height = inner_area.height as usize;
    let total_lines = popup.content.len();
    let scroll = popup.scroll.min(total_lines.saturating_sub(visible_height));

    let visible_content: Vec<Line> = popup
        .content
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(idx, s)| {
            if popup.popup_type == PopupType::Help {
                // Parse styled help content: "KEY|DESCRIPTION|COLOR"
                format_help_line(s)
            } else if popup.popup_type == PopupType::Ignored && idx >= 2 {
                // Highlight selected item in ignored popup (skip header)
                if idx == popup.selected {
                    Line::from(Span::styled(
                        format!("> {}", s),
                        Style::default().fg(Color::Yellow),
                    ))
                } else {
                    Line::from(format!("  {}", s))
                }
            } else {
                Line::from(s.clone())
            }
        })
        .collect();

    let paragraph = Paragraph::new(visible_content);
    f.render_widget(paragraph, inner_area);

    // Draw scrollbar if content overflows
    if total_lines > visible_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);

        f.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut scrollbar_state,
        );
    }
}

fn format_help_line(s: &str) -> Line<'static> {
    if s.is_empty() {
        return Line::from("");
    }

    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 2 {
        return Line::from(s.to_string());
    }

    let key = parts[0];
    let desc = parts[1];
    let color_name = parts.get(2).unwrap_or(&"");

    // Parse color
    let color = match *color_name {
        "cyan" => Some(Color::Cyan),
        "magenta" => Some(Color::Magenta),
        "yellow" => Some(Color::Yellow),
        "green" => Some(Color::Green),
        "red" => Some(Color::Red),
        "blue" => Some(Color::Blue),
        _ => None,
    };

    // Header line
    if key == "HEADER" {
        return Line::from(vec![
            Span::styled(
                desc.to_string(),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]);
    }

    // Regular key-value line
    let mut spans = Vec::new();

    if !key.is_empty() {
        let key_style = color
            .map(|c| Style::default().fg(c))
            .unwrap_or_else(|| Style::default().fg(Color::White));
        spans.push(Span::styled(format!("{:10}", key), key_style));
    } else {
        spans.push(Span::raw("          "));
    }

    spans.push(Span::raw(" "));

    let desc_style = color
        .map(|c| Style::default().fg(c))
        .unwrap_or_else(Style::default);
    spans.push(Span::styled(desc.to_string(), desc_style));

    Line::from(spans)
}

fn draw_confirm_delete_popup(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 25, f.area());
    f.render_widget(Clear, area);

    let (title, warning_text) = match app.pending_delete {
        Some(DeleteType::LocalRepo) => (
            " Confirm Delete Local ",
            "Type 'y' or 'yes' to delete this repository locally:",
        ),
        Some(DeleteType::RemoteRepo) => (
            " Confirm Delete Remote ",
            "Type 'y' or 'yes' to DELETE THIS REPO FROM GITHUB:",
        ),
        Some(DeleteType::Gist) => (
            " Confirm Delete Gist ",
            "Type 'y' or 'yes' to delete this gist from GitHub:",
        ),
        None => (" Confirm Delete ", "Type 'y' or 'yes' to confirm:"),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    let warning = Paragraph::new(warning_text).style(Style::default().fg(Color::Red));
    f.render_widget(warning, chunks[0]);

    let input = Paragraph::new(app.confirm_buffer.as_str())
        .style(Style::default().fg(Color::Red))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(input, chunks[1]);
}

fn draw_upload_form_popup(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 50, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Upload to GitHub ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(ref form) = app.upload_form {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Name
                Constraint::Length(3), // Description
                Constraint::Length(1), // Private
                Constraint::Length(1), // Org
                Constraint::Min(1),    // Instructions
            ])
            .margin(1)
            .split(inner);

        // Name field
        let name_style = if form.active_field == UploadField::Name {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let name_block = Block::default()
            .title(" Name ")
            .borders(Borders::ALL)
            .border_style(name_style);
        let name_input = Paragraph::new(form.name.as_str())
            .block(name_block);
        f.render_widget(name_input, chunks[0]);

        // Description field
        let desc_style = if form.active_field == UploadField::Description {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let desc_block = Block::default()
            .title(" Description (optional) ")
            .borders(Borders::ALL)
            .border_style(desc_style);
        let desc_input = Paragraph::new(form.description.as_str())
            .block(desc_block);
        f.render_widget(desc_input, chunks[1]);

        // Private toggle
        let priv_style = if form.active_field == UploadField::Private {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let private_text = if form.private { "◉ Private" } else { "○ Public" };
        let private_line = Line::from(vec![
            Span::styled("Visibility: ", Style::default()),
            Span::styled(private_text, priv_style),
            Span::styled(" (←/→ to toggle)", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(private_line), chunks[2]);

        // Org selection
        let org_style = if form.active_field == UploadField::Org {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let org_text = if form.selected_org == 0 {
            "Personal account".to_string()
        } else {
            form.orgs.get(form.selected_org - 1)
                .cloned()
                .unwrap_or_else(|| "?".to_string())
        };
        let org_line = Line::from(vec![
            Span::styled("Owner:      ", Style::default()),
            Span::styled(org_text, org_style),
            Span::styled(" (←/→ to change)", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(org_line), chunks[3]);

        // Instructions
        let instr = Line::from(vec![
            Span::styled("Tab/↓↑: navigate │ Enter: submit │ Esc: cancel", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(instr), chunks[4]);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
