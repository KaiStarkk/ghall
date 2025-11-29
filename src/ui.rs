use crate::app::{App, GistRow, InputMode, PopupType, RepoRow, RepoType, ViewMode};
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
        Constraint::Length(14),  // Origin
        Constraint::Length(20),  // Repository
        Constraint::Length(18),  // Type (includes upstream owner for forks)
        Constraint::Length(3),   // Dirty indicator
        Constraint::Length(10),  // Status (ahead/behind)
        Constraint::Min(20),     // Path
    ];

    // Header
    let header = Row::new(vec![
        Cell::from("Origin").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Repository").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Type").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("").style(Style::default().add_modifier(Modifier::BOLD)), // Dirty
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Path").style(Style::default().add_modifier(Modifier::BOLD)),
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
                Cell::from(format_origin(repo)),
                Cell::from(format_repo_name(repo)),
                Cell::from(format_type(repo)),
                Cell::from(format_dirty(repo)),
                Cell::from(format_status(repo)),
                Cell::from(format_path(repo)),
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
    match repo.repo_type() {
        RepoType::Fork => {
            // Fork symbol in purple + upstream owner
            let mut spans = vec![Span::styled("⑂ ", Style::default().fg(Color::Magenta))];
            if let Some(parent_owner) = repo.fork_owner() {
                spans.push(Span::styled(
                    truncate(parent_owner, 14),
                    Style::default().fg(Color::Magenta),
                ));
            }
            Line::from(spans)
        }
        RepoType::Source => {
            // Source indicator (original repo)
            Line::from(Span::styled("● src", Style::default().fg(Color::Green)))
        }
        RepoType::Clone => {
            // Local clone
            Line::from(Span::styled("◌ local", Style::default().fg(Color::Blue)))
        }
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

fn format_status(repo: &RepoRow) -> Span<'static> {
    match &repo.git_status {
        Some(status) => {
            if !status.has_remote {
                return Span::styled("?", Style::default().fg(Color::Blue));
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
    if let Some(ref popup) = app.popup {
        return match popup.popup_type {
            PopupType::Details => "Enter/Esc: close".to_string(),
            PopupType::Ignored => "j/k/↑/↓: select │ Enter: unhide │ Esc: close".to_string(),
            PopupType::Diff => "j/k/↑/↓: scroll │ c: commit & push │ Esc: close".to_string(),
            _ => "j/k/↑/↓: scroll │ Esc: close".to_string(),
        };
    }

    match app.view_mode {
        ViewMode::Repos => {
            let mut parts = vec!["↑/↓/j/k: nav", "g: gists", "Enter: details"];

            // Dynamic actions based on selection
            if let Some(repo) = app.get_selected_repo() {
                if repo.is_remote_only() {
                    parts.push("n: clone");
                }
                if repo.has_local() {
                    parts.push("l: pull");
                    parts.push("h: push");
                    parts.push("s: sync");
                    parts.push("f: diff");
                    parts.push("d: del");
                }
                if app.can_change_visibility(repo) {
                    parts.push("p: priv");
                }
                if repo.is_local_only() {
                    parts.push("u: upload");
                }
            }
            parts.push("i: ignore");
            parts.push("?");

            parts.join(" │ ")
        }
        ViewMode::Gists => {
            let mut parts = vec!["↑/↓/j/k: nav", "g: repos", "Enter: details"];

            if let Some(gist) = app.get_selected_gist() {
                if gist.local_path.is_none() {
                    parts.push("n: clone");
                }
                if gist.has_local() {
                    parts.push("l: pull");
                    parts.push("h: push");
                    parts.push("s: sync");
                    parts.push("f: diff");
                }
            }
            parts.push("d: delete");
            parts.push("r: refresh");
            parts.push("?");

            parts.join(" │ ")
        }
    }
}

fn draw_popup(f: &mut Frame, popup: &crate::app::Popup) {
    let (width, height) = match popup.popup_type {
        PopupType::Help => (55, 70),
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
            } else if popup.popup_type == PopupType::Help {
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
