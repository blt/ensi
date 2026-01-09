//! Replay command implementation.

use super::{CliError, ReplayFormat};
use ensi::replay::{render_ascii, render_llm, Recording, ReplayEngine};
use std::path::PathBuf;

/// Execute the replay command.
///
/// # Errors
///
/// Returns an error if the replay fails.
pub(crate) fn execute(
    recording_path: PathBuf,
    format: ReplayFormat,
    turn: Option<u32>,
    player: Option<u8>,
) -> Result<(), CliError> {
    // Load recording
    let recording = Recording::load(&recording_path).map_err(|e| {
        CliError::new(format!("Failed to load recording {}: {e}", recording_path.display()))
    })?;

    // Create replay engine
    let engine = if let Some(target_turn) = turn {
        ReplayEngine::new_at_turn(recording, target_turn)?
    } else {
        ReplayEngine::new(recording)?
    };

    match format {
        ReplayFormat::Tui => {
            // Re-use watch TUI with the recording
            run_replay_tui(engine, player)
        }
        ReplayFormat::Text => {
            // Output ASCII rendering for each turn
            print_text_replay(engine)
        }
        ReplayFormat::Llm => {
            // Output LLM format for each turn
            print_llm_replay(engine)
        }
    }
}

fn run_replay_tui(engine: ReplayEngine, player: Option<u8>) -> Result<(), CliError> {
    use crossterm::{
        event::{self, Event, KeyCode, KeyEventKind},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io::stdout;
    use std::time::Duration;

    // Create dummy bot names from player count
    let num_players = engine.recording().programs.len();
    let bot_names: Vec<String> = (1..=num_players)
        .map(|i| format!("Player {i}"))
        .collect();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| CliError::new(e.to_string()))?;

    struct ReplayApp {
        engine: ReplayEngine,
        #[allow(dead_code)]
        bot_names: Vec<String>,
        player_view: Option<u8>,
    }

    let mut app = ReplayApp {
        engine,
        bot_names,
        player_view: player,
    };

    loop {
        // Draw using similar rendering as watch
        terminal.draw(|f| {
            use ratatui::{
                layout::{Constraint, Direction, Layout},
                style::{Color, Modifier, Style},
                widgets::{Block, Borders, Paragraph, Wrap},
            };

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(3),
                ])
                .split(f.area());

            // Header
            let turn = app.engine.turn();
            let max_turns = app.engine.recording().config.max_turns;
            let status = if app.engine.is_game_over() { "GAME OVER" } else { "REPLAY" };
            let title = format!(" Ensi Replay | Turn {}/{} | {} ", turn, max_turns, status);
            let header = Paragraph::new(title)
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(header, chunks[0]);

            // Map (simplified ASCII)
            let ascii = render_ascii(app.engine.state(), app.engine.turn());
            let map_widget = Paragraph::new(ascii)
                .block(Block::default().borders(Borders::ALL).title(" Map "))
                .wrap(Wrap { trim: false });
            f.render_widget(map_widget, chunks[1]);

            // Footer
            let controls = " [q] Quit  [←/→] Step  [1-8] Player view ";
            let footer = Paragraph::new(controls)
                .style(Style::default().fg(Color::Gray))
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(footer, chunks[2]);
        }).map_err(|e| CliError::new(e.to_string()))?;

        // Handle input
        if event::poll(Duration::from_millis(100)).map_err(|e| CliError::new(e.to_string()))? {
            if let Event::Key(key) = event::read().map_err(|e| CliError::new(e.to_string()))? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Right | KeyCode::Char('l') => {
                            let _ = app.engine.step_forward();
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            let _ = app.engine.step_backward();
                        }
                        KeyCode::Char('1'..='8') => {
                            let num = key.code.to_string().parse::<u8>().ok();
                            app.player_view = if app.player_view == num { None } else { num };
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}

fn print_text_replay(mut engine: ReplayEngine) -> Result<(), CliError> {
    println!("Replay of game (seed: {})", engine.recording().seed);
    println!("Max turns: {}", engine.recording().config.max_turns);
    println!();

    loop {
        println!("=== Turn {} ===", engine.turn());
        println!("{}", render_ascii(engine.state(), engine.turn()));
        println!();

        if engine.is_game_over() {
            println!("=== GAME OVER ===");
            break;
        }

        if let Err(e) = engine.step_forward() {
            if matches!(e, ensi::replay::ReplayError::GameOver) {
                println!("=== GAME OVER ===");
                break;
            }
            return Err(e.into());
        }
    }

    Ok(())
}

fn print_llm_replay(mut engine: ReplayEngine) -> Result<(), CliError> {
    println!("# Game Replay");
    println!("Seed: {}", engine.recording().seed);
    println!("Max turns: {}", engine.recording().config.max_turns);
    println!();

    loop {
        println!("{}", render_llm(engine.state(), engine.turn()));
        println!();
        println!("---");
        println!();

        if engine.is_game_over() {
            println!("# GAME OVER");
            break;
        }

        if let Err(e) = engine.step_forward() {
            if matches!(e, ensi::replay::ReplayError::GameOver) {
                println!("# GAME OVER");
                break;
            }
            return Err(e.into());
        }
    }

    Ok(())
}
