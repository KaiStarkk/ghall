mod app;
mod git;
mod github;
mod local;
mod ui;

use anyhow::Result;
use app::{App, InputMode, PopupType, ViewMode};
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
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

    // Create app and run
    let mut app = App::new(path).await?;
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

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll for events with timeout to allow async updates
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.input_mode {
                        InputMode::Normal => {
                            if !handle_normal_mode(app, key.code, key.modifiers).await? {
                                return Ok(());
                            }
                        }
                        InputMode::Commit => {
                            handle_commit_mode(app, key.code).await?;
                        }
                        InputMode::ConfirmDelete => {
                            handle_confirm_delete_mode(app, key.code).await?;
                        }
                    }
                }
            }
        }
    }
}

async fn handle_normal_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<bool> {
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
            _ => {
                match code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.close_popup(),
                    KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
                    KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
                    KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.scroll_down(),
                    KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => app.scroll_up(),
                    _ => {}
                }
            }
        }
        return Ok(true);
    }

    // Normal navigation and commands
    match code {
        // Quit
        KeyCode::Char('q') | KeyCode::Esc => return Ok(false),

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => app.next(),
        KeyCode::Char('k') | KeyCode::Up => app.previous(),

        // View mode toggle (lowercase g)
        KeyCode::Char('g') => app.toggle_view_mode(),

        // Help
        KeyCode::Char('?') => app.toggle_help(),

        // Refresh
        KeyCode::Char('r') => app.refresh().await?,

        // Details popup
        KeyCode::Enter => app.show_details(),

        // Mode-specific actions
        _ => {
            match app.view_mode {
                ViewMode::Repos => handle_repos_action(app, code).await?,
                ViewMode::Gists => handle_gists_action(app, code).await?,
            }
        }
    }

    Ok(true)
}

async fn handle_repos_action(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        // Clone remote-only repo (n for new/clone)
        KeyCode::Char('n') => {
            app.clone_selected().await?;
        }

        // Git operations
        KeyCode::Char('l') => app.pull_selected().await?,
        KeyCode::Char('h') => app.push_selected().await?,  // h for push (changed from p)
        KeyCode::Char('s') => app.sync_selected().await?,

        // Commit dirty files
        KeyCode::Char('c') => {
            let is_dirty = app.get_selected_repo()
                .and_then(|r| r.git_status.as_ref())
                .map(|s| s.is_dirty())
                .unwrap_or(false);
            if is_dirty {
                app.start_commit();
            }
        }

        // Show diff (f for diff)
        KeyCode::Char('f') => app.show_diff().await?,

        // Toggle private/public (p)
        KeyCode::Char('p') => app.toggle_private().await?,

        // Delete local copy (d for delete)
        KeyCode::Char('d') => {
            let has_local = app.get_selected_repo()
                .map(|r| r.has_local())
                .unwrap_or(false);
            if has_local {
                app.start_delete_confirm();
            }
        }

        // Ignore/hide repo
        KeyCode::Char('i') => app.toggle_ignore(),

        // Show ignored repos popup
        KeyCode::Char('I') => app.show_ignored_popup(),

        _ => {}
    }
    Ok(())
}

async fn handle_gists_action(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        // Clone gist (n for new/clone)
        KeyCode::Char('n') => {
            app.clone_gist().await?;
        }

        // Delete gist (d for delete)
        KeyCode::Char('d') => {
            app.start_gist_delete_confirm();
        }

        _ => {}
    }
    Ok(())
}

async fn handle_commit_mode(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => app.close_popup(),
        KeyCode::Enter => {
            app.commit_and_push().await?;
        }
        KeyCode::Char(c) => app.handle_char(c),
        KeyCode::Backspace => app.handle_backspace(),
        _ => {}
    }
    Ok(())
}

async fn handle_confirm_delete_mode(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => app.close_popup(),
        KeyCode::Enter => {
            match app.view_mode {
                ViewMode::Repos => app.delete_local_repo().await?,
                ViewMode::Gists => app.delete_gist().await?,
            }
        }
        KeyCode::Char(c) => app.handle_char(c),
        KeyCode::Backspace => app.handle_backspace(),
        _ => {}
    }
    Ok(())
}
