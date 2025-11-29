use crate::app::{App, GistRow, InputMode, PopupType, RepoRow, ViewMode};
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

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title bar with mode tabs
            Constraint::Min(0),    // Main content (table)
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Title bar with view mode tabs
    draw_title_bar(f, chunks[0], app);

    // Main content - table
    match app.view_mode {
        ViewMode::Repos => draw_repos_table(f, chunks[1], app),
        ViewMode::Gists => draw_gists_table(f, chunks[1], app),
    }

    // Status bar
    let status = if let Some(ref msg) = app.status_message {
        msg.clone()
    } else {
        get_status_bar_text(app)
    };
    let status_bar = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
    f.render_widget(status_bar, chunks[2]);

    // Draw popups/input modes
    match app.input_mode {
        InputMode::Commit => draw_commit_popup(f, app),
        InputMode::ConfirmDelete => draw_confirm_delete_popup(f, app),
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
        Span::styled("[R]epos", repos_style),
        Span::raw("  "),
        Span::styled("[g]ists", gists_style),
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

    // Column widths - pack left with fixed widths, Path takes remainder
    let widths = [
        Constraint::Length(14),  // Owner
        Constraint::Length(20),  // Repository
        Constraint::Length(12),  // Forked from
        Constraint::Length(10),  // Status (icon + ahead/behind)
        Constraint::Min(20),     // Path
        Constraint::Length(10),  // Remote
    ];

    // Header
    let header = Row::new(vec![
        Cell::from("Owner").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Repository").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Forked from").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Path").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Remote").style(Style::default().add_modifier(Modifier::BOLD)),
    ])
    .style(Style::default().fg(Color::Cyan))
    .height(1);

    // Rows
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

            Row::new(vec![
                Cell::from(format_owner(repo)),
                Cell::from(format_repo_name(repo)),
                Cell::from(format_fork_parent(repo)),
                Cell::from(format_status(repo)),
                Cell::from(format_path(repo)),
                Cell::from(format_remote(repo)),
            ])
            .style(row_style)
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
        Constraint::Length(8),   // Files
        Constraint::Length(8),   // Public
        Constraint::Length(25),  // Local
    ];

    // Header
    let header = Row::new(vec![
        Cell::from("Description").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Files").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Public").style(Style::default().add_modifier(Modifier::BOLD)),
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
fn format_owner(repo: &RepoRow) -> Span<'static> {
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

fn format_fork_parent(repo: &RepoRow) -> Span<'static> {
    if let Some(parent_owner) = repo.fork_owner() {
        let display = truncate(parent_owner, 11);
        Span::styled(display, Style::default().fg(Color::Magenta))
    } else {
        Span::raw("")
    }
}

fn format_status(repo: &RepoRow) -> Span<'static> {
    match &repo.git_status {
        Some(status) => {
            let icon = status.status_icon();
            let extra = if status.ahead > 0 && status.behind > 0 {
                format!("{} +{}/-{}", icon, status.ahead, status.behind)
            } else if status.ahead > 0 {
                format!("{} +{}", icon, status.ahead)
            } else if status.behind > 0 {
                format!("{} -{}", icon, status.behind)
            } else {
                icon.to_string()
            };
            let color = get_status_color(repo);
            Span::styled(extra, Style::default().fg(color))
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

fn format_remote(repo: &RepoRow) -> Span<'static> {
    if repo.github_url.is_some() {
        let mut parts = Vec::new();
        parts.push("✓");

        if repo.is_private {
            parts.push(" priv");
        }

        Span::styled(parts.join(""), Style::default().fg(Color::Green))
    } else {
        Span::styled("—", Style::default().fg(Color::DarkGray))
    }
}

fn get_status_color(repo: &RepoRow) -> Color {
    if let Some(ref status) = repo.git_status {
        if status.is_dirty() {
            Color::Yellow
        } else if status.ahead > 0 && status.behind > 0 {
            Color::Red
        } else if status.ahead > 0 {
            Color::Magenta
        } else if status.behind > 0 {
            Color::Cyan
        } else if !status.has_remote {
            Color::Blue
        } else {
            Color::Green
        }
    } else {
        Color::White
    }
}

// Formatting helpers for gists table
fn format_gist_description(gist: &GistRow) -> Span<'static> {
    let desc = if gist.description.is_empty() {
        gist.file_names.first().cloned().unwrap_or_else(|| "Untitled".to_string())
    } else {
        gist.description.clone()
    };
    Span::raw(truncate(&desc, 40))
}

fn format_gist_local(gist: &GistRow) -> Span<'static> {
    match &gist.local_path {
        Some(path) => {
            let short = shorten_path(path);
            Span::styled(truncate(&short, 23), Style::default().fg(Color::Green))
        }
        None => Span::styled("—", Style::default().fg(Color::DarkGray)),
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

fn get_status_bar_text(app: &App) -> String {
    if app.popup.is_some() {
        return "j/k: scroll │ Esc: close │ Enter: select".to_string();
    }

    match app.view_mode {
        ViewMode::Repos => {
            "j/k: nav │ g: gists │ Enter: details │ n: clone │ l: pull │ h: push │ s: sync │ c: commit │ f: diff │ d: del │ p: priv │ i: ignore │ ?".to_string()
        }
        ViewMode::Gists => {
            "j/k: nav │ g: repos │ Enter: details │ n: clone │ d: delete │ r: refresh │ ?: help │ q: quit".to_string()
        }
    }
}

fn draw_popup(f: &mut Frame, popup: &crate::app::Popup) {
    let (width, height) = match popup.popup_type {
        PopupType::Help => (60, 70),
        PopupType::DirtyFiles => (60, 50),
        PopupType::Diff => (80, 90),
        PopupType::Details => (60, 50),
        PopupType::Ignored => (60, 50),
    };

    let area = centered_rect(width, height, f.area());
    f.render_widget(Clear, area);

    let title = match popup.popup_type {
        PopupType::Help => " Help ",
        PopupType::DirtyFiles => " Dirty Files ",
        PopupType::Diff => " Diff ",
        PopupType::Details => " Details ",
        PopupType::Ignored => " Ignored Repos ",
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
            // Syntax highlighting for diff
            if popup.popup_type == PopupType::Diff {
                if s.starts_with('+') && !s.starts_with("+++") {
                    Line::from(Span::styled(s.clone(), Style::default().fg(Color::Green)))
                } else if s.starts_with('-') && !s.starts_with("---") {
                    Line::from(Span::styled(s.clone(), Style::default().fg(Color::Red)))
                } else if s.starts_with("@@") {
                    Line::from(Span::styled(s.clone(), Style::default().fg(Color::Cyan)))
                } else if s.starts_with("===") {
                    Line::from(Span::styled(
                        s.clone(),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ))
                } else {
                    Line::from(s.clone())
                }
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

fn draw_commit_popup(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 30, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Commit & Push ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    let label = Paragraph::new("Enter commit message (Enter to confirm, Esc to cancel):");
    f.render_widget(label, chunks[0]);

    let input = Paragraph::new(app.input_buffer.as_str())
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(input, chunks[1]);
}

fn draw_confirm_delete_popup(f: &mut Frame, app: &App) {
    let area = centered_rect(50, 25, f.area());
    f.render_widget(Clear, area);

    let title = match app.view_mode {
        ViewMode::Repos => " Confirm Delete Local ",
        ViewMode::Gists => " Confirm Delete Gist ",
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

    let warning_text = match app.view_mode {
        ViewMode::Repos => "Type 'y' or 'yes' to delete this repository locally:",
        ViewMode::Gists => "Type 'y' or 'yes' to delete this gist from GitHub:",
    };

    let warning = Paragraph::new(warning_text).style(Style::default().fg(Color::Red));
    f.render_widget(warning, chunks[0]);

    let input = Paragraph::new(app.confirm_buffer.as_str())
        .style(Style::default().fg(Color::Red))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(input, chunks[1]);
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
