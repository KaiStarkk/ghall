mod app;
mod config;
mod git;
mod github;
mod local;
mod ui;

use anyhow::Result;
use app::{App, DeleteType, InputMode, PopupType, ViewMode};
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::process::Command;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "ghall")]
#[command(about = "A TUI for managing git repositories across GitHub and local", long_about = None)]
struct Args {
    /// Path to scan for local repositories
    #[arg(short, long, default_value = "~/code")]
    path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Expand ~ in path
    let path = shellexpand::tilde(&args.path).to_string();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run (App::new is now synchronous, refresh happens in event loop)
    let mut app = App::new(path)?;
    let res = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {err:?}");
    }

    Ok(())
}

/// Spawn lazygit in the given repo directory
fn spawn_lazygit<B: Backend>(terminal: &mut Terminal<B>, path: &str) -> Result<()> {
    // Leave TUI mode
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    // Spawn lazygit
    let status = Command::new("lazygit")
        .arg("-p")
        .arg(path)
        .status();

    // Restore TUI mode
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    // Force terminal clear and redraw
    terminal.clear()?;

    // Check if lazygit succeeded
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Ok(()), // lazygit exited with non-zero, but that's fine
        Err(e) => Err(anyhow::anyhow!("Failed to spawn lazygit: {}", e)),
    }
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        // Tick spinner for status feedback
        app.tick_spinner();

        // Check for completed background tasks
        app.poll_tasks();

        // Check for completed refresh data
        app.poll_refresh();

        // Handle pending refresh from background tasks
        // Full refresh takes precedence over local-only refresh
        if app.pending_refresh {
            app.pending_refresh = false;
            app.pending_local_refresh = false; // Full refresh supersedes local
            app.trigger_refresh();
        } else if app.pending_local_refresh {
            app.pending_local_refresh = false;
            app.trigger_local_refresh();
        }

        terminal.draw(|f| ui::draw(f, app))?;

        // Poll for events with timeout to allow async updates
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        match app.input_mode {
                            InputMode::Normal => {
                                if !handle_normal_mode(terminal, app, key.code, key.modifiers).await? {
                                    return Ok(());
                                }
                            }
                            InputMode::ConfirmDelete => {
                                handle_confirm_delete_mode(app, key.code);
                            }
                            InputMode::UploadForm => {
                                handle_upload_form_mode(app, key.code);
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if app.input_mode == InputMode::Normal && app.popup.is_none() {
                        match mouse.kind {
                            MouseEventKind::Down(_) => {
                                app.handle_mouse_click(mouse.row, mouse.column);
                            }
                            MouseEventKind::ScrollDown => {
                                app.next();
                            }
                            MouseEventKind::ScrollUp => {
                                app.previous();
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

async fn handle_normal_mode<B: Backend>(terminal: &mut Terminal<B>, app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
    // If popup is open, handle popup navigation
    if let Some(ref popup) = app.popup {
        match popup.popup_type {
            PopupType::Ignored => {
                match code {
                    KeyCode::Esc | KeyCode::Char('q') => app.close_popup(),
                    KeyCode::Char('j') | KeyCode::Down => app.popup_next(),
                    KeyCode::Char('k') | KeyCode::Up => app.popup_prev(),
                    KeyCode::Enter => app.unhide_selected_in_popup(),
                    _ => {}
                }
            }
            PopupType::Details => {
                // Details popup - Enter or Esc closes
                match code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => app.close_popup(),
                    _ => {}
                }
            }
            _ => {
                match code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.close_popup(),
                    KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
                    KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
                    KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.scroll_down(),
                    KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => app.scroll_up(),
                    KeyCode::Char('y') => app.copy_popup_to_clipboard(),
                    _ => {}
                }
            }
        }
        return Ok(true);
    }

    // Normal navigation and commands
    match code {
        // Quit
        KeyCode::Esc | KeyCode::Char('q') => return Ok(false),

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => app.next(),
        KeyCode::Char('k') | KeyCode::Up => app.previous(),

        // Sorting column change and direction
        KeyCode::Left => app.prev_sort_column(),
        KeyCode::Right => app.next_sort_column(),
        KeyCode::Char('v') => app.toggle_sort_direction(),

        // Column reordering (< > move column, , . select column)
        KeyCode::Char('<') => app.move_column_left(),
        KeyCode::Char('>') => app.move_column_right(),
        KeyCode::Char(',') => app.select_prev_column(),
        KeyCode::Char('.') => app.select_next_column(),

        // View mode toggle (Tab key)
        KeyCode::Tab => app.toggle_view_mode(),

        // Help
        KeyCode::Char('?') => app.toggle_help(),

        // Error log
        KeyCode::Char('E') => app.show_error_log(),

        // Refresh
        KeyCode::Char('r') => app.trigger_refresh(),

        // Details popup
        KeyCode::Enter => app.show_details(),

        // Toggle show archived (capital A)
        KeyCode::Char('A') => app.toggle_show_archived(),

        // Toggle show private (capital P)
        KeyCode::Char('P') => app.toggle_show_private(),

        // Mark/unmark item for batch operations
        KeyCode::Char('x') => app.toggle_mark(),

        // Clear all marks
        KeyCode::Char('X') => app.clear_marks(),

        // Mode-specific actions
        _ => {
            match app.view_mode {
                ViewMode::Repos => {
                    if let Some(lazygit_path) = handle_repos_action(app, code).await? {
                        spawn_lazygit(terminal, &lazygit_path)?;
                        app.trigger_refresh();
                    }
                }
                ViewMode::Gists => handle_gists_action(app, code).await?,
            }
        }
    }

    Ok(true)
}

/// Returns Some(path) if lazygit should be opened at that path
async fn handle_repos_action(app: &mut App, code: KeyCode) -> Result<Option<String>> {
    match code {
        // Clone remote-only repo (n for new/clone)
        KeyCode::Char('n') => {
            let is_remote_only = app.get_selected_repo()
                .map(|r| r.is_remote_only())
                .unwrap_or(false);
            if is_remote_only {
                app.clone_selected();
            }
        }

        // Git operations (only if has local) - spawned as background tasks
        KeyCode::Char('l') => {
            let has_local = app.get_selected_repo().map(|r| r.has_local()).unwrap_or(false);
            if has_local {
                app.pull_selected();
            }
        }
        KeyCode::Char('h') => {
            let has_local = app.get_selected_repo().map(|r| r.has_local()).unwrap_or(false);
            if has_local {
                app.push_selected();
            }
        }
        KeyCode::Char('s') => {
            let has_local = app.get_selected_repo().map(|r| r.has_local()).unwrap_or(false);
            if has_local {
                app.sync_selected();
            }
        }

        // Quicksync (y) - fetch, rebase, add, commit fixup, push
        KeyCode::Char('y') => {
            let has_local = app.get_selected_repo().map(|r| r.has_local()).unwrap_or(false);
            if has_local {
                app.quicksync_selected();
            }
        }

        // Open lazygit (g) - only if has local
        KeyCode::Char('g') => {
            if let Some(path) = app.get_selected_repo().and_then(|r| r.local_path.clone()) {
                return Ok(Some(path));
            }
        }

        // Toggle private/public (p) - only if user owns the repo
        KeyCode::Char('p') => {
            let can_change = app.get_selected_repo()
                .map(|r| app.can_change_visibility(r))
                .unwrap_or(false);
            if can_change {
                app.toggle_private();
            }
        }

        // Toggle archived (a) - only if user owns the repo
        KeyCode::Char('a') => {
            let can_change = app.get_selected_repo()
                .map(|r| app.can_change_visibility(r))
                .unwrap_or(false);
            if can_change {
                app.toggle_archived();
            }
        }

        // Open in browser (o)
        KeyCode::Char('o') => {
            if let Some(url) = app.get_selected_repo().and_then(|r| r.github_url.clone()) {
                let _ = Command::new("xdg-open")
                    .arg(&url)
                    .spawn();
            }
        }

        // Open in file manager (O)
        KeyCode::Char('O') => {
            if let Some(path) = app.get_selected_repo().and_then(|r| r.local_path.clone()) {
                let _ = Command::new("xdg-open")
                    .arg(&path)
                    .spawn();
            }
        }

        // Upload local-only repo to GitHub (u)
        KeyCode::Char('u') => {
            let is_local_only = app.get_selected_repo()
                .map(|r| r.is_local_only())
                .unwrap_or(false);
            if is_local_only {
                app.show_upload_form();
            }
        }

        // Delete local copy (d for delete) - only if has local
        KeyCode::Char('d') => {
            let has_local = app.get_selected_repo()
                .map(|r| r.has_local())
                .unwrap_or(false);
            if has_local {
                app.start_delete_confirm();
            }
        }

        // Init repo (if no git) or Ignore/hide repo
        KeyCode::Char('i') => {
            let is_nogit = app.get_selected_repo()
                .map(|r| !r.has_git && r.has_local())
                .unwrap_or(false);
            if is_nogit {
                app.init_repo();
            } else {
                app.toggle_ignore();
            }
        }

        // Show ignored repos popup
        KeyCode::Char('I') => app.show_ignored_popup(),

        // Delete remote repo (D)
        KeyCode::Char('D') => {
            let can_delete = app.get_selected_repo()
                .map(|r| r.github_url.is_some() && r.is_member)
                .unwrap_or(false);
            if can_delete {
                app.start_delete_remote_confirm();
            }
        }

        // Reorganize to ghq path (z)
        KeyCode::Char('z') => {
            let needs_reorg = app.get_selected_repo()
                .map(|r| r.follows_ghq(&app.local_root) == Some(false))
                .unwrap_or(false);
            if needs_reorg {
                app.reorganize_to_ghq();
            }
        }

        _ => {}
    }
    Ok(None)
}

async fn handle_gists_action(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        // Clone gist (n for new/clone)
        KeyCode::Char('n') => {
            let is_remote_only = app.get_selected_gist()
                .map(|g| g.local_path.is_none())
                .unwrap_or(false);
            if is_remote_only {
                app.clone_gist();
            }
        }

        // Git operations (only if has local) - spawned as background tasks
        KeyCode::Char('l') => {
            let has_local = app.get_selected_gist().map(|g| g.has_local()).unwrap_or(false);
            if has_local {
                app.pull_gist();
            }
        }
        KeyCode::Char('h') => {
            let has_local = app.get_selected_gist().map(|g| g.has_local()).unwrap_or(false);
            if has_local {
                app.push_gist();
            }
        }
        KeyCode::Char('s') => {
            let has_local = app.get_selected_gist().map(|g| g.has_local()).unwrap_or(false);
            if has_local {
                app.sync_gist();
            }
        }

        // Delete gist (d for delete)
        KeyCode::Char('d') => {
            app.start_gist_delete_confirm();
        }

        _ => {}
    }
    Ok(())
}

fn handle_confirm_delete_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.pending_delete = None;
            app.close_popup();
        }
        KeyCode::Enter => {
            match app.pending_delete {
                Some(DeleteType::LocalRepo) => app.delete_local_repo(),
                Some(DeleteType::RemoteRepo) => app.delete_remote_repo(),
                Some(DeleteType::Gist) => app.delete_gist(),
                None => app.close_popup(),
            }
        }
        KeyCode::Char(c) => app.handle_char(c),
        KeyCode::Backspace => app.handle_backspace(),
        _ => {}
    }
}

fn handle_upload_form_mode(app: &mut App, code: KeyCode) {
    use app::UploadField;

    match code {
        KeyCode::Esc => {
            app.cancel_upload_form();
        }
        KeyCode::Enter => {
            // If on a text field, move to next field
            // If on last field or pressing Enter on any field, submit
            if let Some(ref form) = app.upload_form {
                match form.active_field {
                    UploadField::Org => app.submit_upload_form(),
                    _ => app.upload_form_next_field(),
                }
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            app.upload_form_next_field();
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.upload_form_prev_field();
        }
        KeyCode::Left => {
            if let Some(ref form) = app.upload_form {
                match form.active_field {
                    UploadField::Private => app.upload_form_toggle_private(),
                    UploadField::Org => app.upload_form_prev_org(),
                    _ => {}
                }
            }
        }
        KeyCode::Right => {
            if let Some(ref form) = app.upload_form {
                match form.active_field {
                    UploadField::Private => app.upload_form_toggle_private(),
                    UploadField::Org => app.upload_form_next_org(),
                    _ => {}
                }
            }
        }
        KeyCode::Char(c) => app.handle_char(c),
        KeyCode::Backspace => app.handle_backspace(),
        _ => {}
    }
}
