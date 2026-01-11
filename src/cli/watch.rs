//! Watch command implementation - Interactive TUI viewer.

// CLI watch uses intentional casts for display and timing
#![allow(
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

use super::CliError;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ensi::game::{GameState, PlayerId};
use ensi::replay::{Recording, ReplayEngine};
use ensi::tournament::{PlayerProgram, TournamentConfig};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::fs;
use std::io::stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Execute the watch command.
///
/// # Errors
///
/// Returns an error if the TUI fails.
pub(crate) fn execute(
    bots: Vec<PathBuf>,
    seed: Option<u64>,
    turns: u32,
    speed: u64,
    player: Option<u8>,
) -> Result<(), CliError> {
    // Load bot programs
    let mut programs = Vec::with_capacity(bots.len());
    let mut bot_names = Vec::with_capacity(bots.len());

    for bot_path in &bots {
        let wasm_bytes = fs::read(bot_path).map_err(|e| {
            CliError::new(format!("Failed to read {}: {e}", bot_path.display()))
        })?;
        programs.push(PlayerProgram::new(wasm_bytes.clone()));
        bot_names.push(
            bot_path
                .file_name().map_or_else(|| "unknown".to_string(), |n| n.to_string_lossy().to_string()),
        );
    }

    // Generate seed if not provided
    let seed = seed.unwrap_or_else(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42)
    });

    // Config
    let config = TournamentConfig {
        max_turns: turns,
        ..TournamentConfig::default()
    };

    // Create recording and replay engine
    let wasm_bytes: Vec<Vec<u8>> = programs.iter().map(|p| p.wasm_bytes.clone()).collect();
    let recording = Recording::new(seed, wasm_bytes, config);
    let engine = ReplayEngine::new(recording)?;

    // Run the TUI
    run_tui(engine, bot_names, speed, player)
}

/// App state for the TUI.
struct App {
    engine: ReplayEngine,
    bot_names: Vec<String>,
    paused: bool,
    speed_ms: u64,
    player_view: Option<PlayerId>,
    last_step: Instant,
}

impl App {
    fn new(engine: ReplayEngine, bot_names: Vec<String>, speed_ms: u64, player_view: Option<u8>) -> Self {
        Self {
            engine,
            bot_names,
            paused: true, // Start paused
            speed_ms,
            player_view,
            last_step: Instant::now(),
        }
    }

    fn step_forward(&mut self) {
        if !self.engine.is_game_over() {
            let _ = self.engine.step_forward();
            self.last_step = Instant::now();
        }
    }

    fn step_backward(&mut self) {
        let _ = self.engine.step_backward();
        self.last_step = Instant::now();
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    fn increase_speed(&mut self) {
        self.speed_ms = self.speed_ms.saturating_sub(100).max(50);
    }

    fn decrease_speed(&mut self) {
        self.speed_ms = (self.speed_ms + 100).min(2000);
    }

    fn should_auto_step(&self) -> bool {
        !self.paused && !self.engine.is_game_over() && self.last_step.elapsed() >= Duration::from_millis(self.speed_ms)
    }
}

fn run_tui(engine: ReplayEngine, bot_names: Vec<String>, speed: u64, player: Option<u8>) -> Result<(), CliError> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| CliError::new(e.to_string()))?;

    let mut app = App::new(engine, bot_names, speed, player);

    loop {
        // Draw
        terminal.draw(|f| ui(f, &app)).map_err(|e| CliError::new(e.to_string()))?;

        // Auto-step if needed
        if app.should_auto_step() {
            app.step_forward();
        }

        // Handle input with timeout
        if event::poll(Duration::from_millis(50)).map_err(|e| CliError::new(e.to_string()))?
            && let Event::Key(key) = event::read().map_err(|e| CliError::new(e.to_string()))?
                && key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char(' ') => app.toggle_pause(),
                        KeyCode::Right | KeyCode::Char('l') => {
                            app.paused = true;
                            app.step_forward();
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            app.paused = true;
                            app.step_backward();
                        }
                        KeyCode::Char('+' | '=') => app.increase_speed(),
                        KeyCode::Char('-') => app.decrease_speed(),
                        KeyCode::Char('r') => {
                            let _ = app.engine.goto_turn(0);
                            app.paused = true;
                        }
                        KeyCode::Char('1'..='8') => {
                            let num = key.code.to_string().parse::<u8>().ok();
                            app.player_view = if app.player_view == num { None } else { num };
                        }
                        _ => {}
                    }
                }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Main content
            Constraint::Length(3),  // Footer
        ])
        .split(f.area());

    // Header
    render_header(f, chunks[0], app);

    // Main content - map and stats
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(chunks[1]);

    render_map(f, main_chunks[0], app);
    render_stats(f, main_chunks[1], app);

    // Footer
    render_footer(f, chunks[2], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let turn = app.engine.turn();
    let max_turns = app.engine.recording().config.max_turns;

    let status = if app.engine.is_game_over() {
        "GAME OVER"
    } else if app.paused {
        "PAUSED"
    } else {
        "RUNNING"
    };

    let title = format!(
        " Ensi Game Viewer | Turn {}/{} | {} | Speed: {}ms ",
        turn, max_turns, status, app.speed_ms
    );

    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(header, area);
}

fn render_map(f: &mut Frame, area: Rect, app: &App) {
    let state = app.engine.state();
    let map = &state.map;

    let mut lines: Vec<Line> = Vec::new();

    // Simple ASCII rendering - show a portion of the map that fits
    let visible_width = (area.width as usize).saturating_sub(4).min(map.width() as usize);
    let visible_height = (area.height as usize).saturating_sub(2).min(map.height() as usize);

    for y in 0..visible_height {
        let mut spans = Vec::new();
        for x in 0..visible_width {
            let coord = ensi::Coord::new(x as u16, y as u16);
            if let Some(tile) = map.get(coord) {
                let (ch, color) = tile_to_char_color(tile, app.player_view);
                spans.push(Span::styled(ch, Style::default().fg(color)));
            } else {
                spans.push(Span::raw(" "));
            }
        }
        lines.push(Line::from(spans));
    }

    let map_widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Map "));

    f.render_widget(map_widget, area);
}

fn tile_to_char_color(tile: &ensi::Tile, _player_view: Option<PlayerId>) -> (String, Color) {
    use ensi::game::TileType;

    let color = match tile.owner {
        Some(1) => Color::Red,
        Some(2) => Color::Blue,
        Some(3) => Color::Green,
        Some(4) => Color::Yellow,
        Some(5) => Color::Magenta,
        Some(6) => Color::Cyan,
        Some(7) => Color::LightRed,
        Some(8) => Color::LightBlue,
        Some(_) | None => Color::DarkGray,
    };

    let ch = match tile.tile_type {
        TileType::Mountain => "M".to_string(),
        TileType::City => {
            if tile.army > 0 {
                format!("{}", (tile.army % 10))
            } else {
                "C".to_string()
            }
        }
        TileType::Desert => {
            if tile.army > 0 {
                format!("{}", (tile.army % 10))
            } else {
                ".".to_string()
            }
        }
    };

    (ch, color)
}

fn render_stats(f: &mut Frame, area: Rect, app: &App) {
    let state = app.engine.state();
    let mut lines = Vec::new();

    lines.push(Line::from(""));

    for (i, player) in state.players.iter().enumerate() {
        let name = app.bot_names.get(i).map_or("Unknown", String::as_str);
        let color = player_color(player.id);

        let status = if player.alive {
            ""
        } else {
            " [ELIMINATED]"
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("Player {} ", player.id),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("({name}){status}")),
        ]));

        if player.alive {
            // Count cities and calculate stats
            let (cities, pop, army, food) = calculate_player_stats(state, player.id);
            let score = state.calculate_score(player.id);

            lines.push(Line::from(format!("  Cities: {cities}")));
            lines.push(Line::from(format!("  Pop: {pop}  Army: {army}")));
            lines.push(Line::from(format!("  Food: {food:+}/turn")));
            lines.push(Line::from(format!("  Score: {score:.0}")));
        }
        lines.push(Line::from(""));
    }

    let stats_widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Players "))
        .wrap(Wrap { trim: false });

    f.render_widget(stats_widget, area);
}

fn calculate_player_stats(state: &GameState, player_id: PlayerId) -> (u32, u32, u32, i32) {
    let mut cities = 0u32;
    let mut pop = 0u32;
    let mut army = 0u32;

    for y in 0..state.map.height() {
        for x in 0..state.map.width() {
            let coord = ensi::Coord::new(x, y);
            if let Some(tile) = state.map.get(coord)
                && tile.owner == Some(player_id) {
                    army += tile.army;
                    if matches!(tile.tile_type, ensi::game::TileType::City) {
                        cities += 1;
                        pop += tile.population;
                    }
                }
        }
    }

    // Food = pop - army (each pop produces 2, consumes 1; each army consumes 1)
    let food = pop as i32 - army as i32;

    (cities, pop, army, food)
}

fn player_color(id: PlayerId) -> Color {
    match id {
        1 => Color::Red,
        2 => Color::Blue,
        3 => Color::Green,
        4 => Color::Yellow,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::LightRed,
        8 => Color::LightBlue,
        _ => Color::White,
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let controls = if app.engine.is_game_over() {
        " [q] Quit  [r] Restart  [←/→] Step  [1-8] Player view "
    } else {
        " [q] Quit  [Space] Pause  [←/→] Step  [+/-] Speed  [r] Restart  [1-8] Player view "
    };

    let footer = Paragraph::new(controls)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}
