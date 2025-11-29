mod app;
mod git;
mod github;
mod local;
mod ui;

use anyhow::Result;
use app::{App, InputMode, ViewMode};
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
    if app.popup.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.close_popup(),
            KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
            KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.scroll_down(),
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => app.scroll_up(),
            _ => {}
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

        // View mode toggle
        KeyCode::Char('G') => app.toggle_view_mode(),

        // Help
        KeyCode::Char('?') => app.toggle_help(),

        // Refresh
        KeyCode::Char('r') => app.refresh().await?,

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
        // Clone remote-only repo
        KeyCode::Enter => {
            app.clone_selected().await?;
        }

        // Git operations (only for repos with local copy)
        KeyCode::Char('f') => app.fetch_selected().await?,
        KeyCode::Char('l') => app.pull_selected().await?,
        KeyCode::Char('p') => app.push_selected().await?,
        KeyCode::Char('s') => app.sync_selected().await?,

        // Commit dirty files
        KeyCode::Char('c') => {
            if let Some(repo) = app.get_selected_repo() {
                if repo.git_status.as_ref().map(|s| s.is_dirty()).unwrap_or(false) {
                    app.show_dirty_files().await?;
                    app.start_commit();
                }
            }
        }

        // Show diff
        KeyCode::Char('d') => app.show_diff().await?,

        // Create GitHub repo for local-only
        KeyCode::Char('g') => {
            if let Some(repo) = app.get_selected_repo() {
                if repo.is_local_only() {
                    app.create_github_repo().await?;
                }
            }
        }

        // Delete local copy (only if synced)
        KeyCode::Char('x') => {
            if let Some(repo) = app.get_selected_repo() {
                if repo.has_local() && repo.git_status.as_ref().map(|s| s.is_synced()).unwrap_or(false) {
                    app.start_delete_confirm();
                }
            }
        }

        _ => {}
    }
    Ok(())
}

async fn handle_gists_action(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        // Clone gist
        KeyCode::Enter => {
            app.clone_gist().await?;
        }

        // Delete gist
        KeyCode::Char('x') => {
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
