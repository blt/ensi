//! Examine evolved genomes from checkpoint files.

use ensi::gp::{load_checkpoint, Expr, Action, TileRef};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).map(|s| s.as_str()).unwrap_or("/tmp/evolution_long/gen_00000.bin");

    let checkpoint = load_checkpoint(Path::new(path))
        .expect("Failed to load checkpoint");

    println!("=== Checkpoint: Generation {} ===\n", checkpoint.generation);
    println!("Population size: {}", checkpoint.population.len());
    println!("Best fitness: {:.4}\n", checkpoint.best_fitness);

    // Find the best genome by fitness
    let best_idx = if !checkpoint.fitness.is_empty() {
        checkpoint.fitness.iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    } else {
        0
    };

    let best = &checkpoint.population[best_idx];
    println!("Best Genome (index {}):", best_idx);
    println!("  Rules: {}", best.rules.len());
    println!("  Constants: {:?}\n", best.constants);

    for (i, rule) in best.rules.iter().enumerate() {
        println!("Rule {} (priority {}):", i, rule.priority);
        println!("  IF {}", format_expr(&rule.condition));
        println!("  THEN {}", format_action(&rule.action));
        println!();
    }
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Const(i) => format!("const[{}]", i),
        Expr::Turn => "turn".to_string(),
        Expr::MyFood => "my_food".to_string(),
        Expr::MyPop => "my_pop".to_string(),
        Expr::MyArmy => "my_army".to_string(),
        Expr::MyTerritory => "my_territory".to_string(),
        Expr::MapWidth => "map_width".to_string(),
        Expr::MapHeight => "map_height".to_string(),
        Expr::IterX => "iter_x".to_string(),
        Expr::IterY => "iter_y".to_string(),
        Expr::Reg(i) => format!("reg[{}]", i),
        Expr::TileType(t) => format!("tile_type({})", format_tile_ref(t)),
        Expr::TileOwner(t) => format!("tile_owner({})", format_tile_ref(t)),
        Expr::TileArmy(t) => format!("tile_army({})", format_tile_ref(t)),
        Expr::Add(a, b) => format!("({} + {})", format_expr(a), format_expr(b)),
        Expr::Sub(a, b) => format!("({} - {})", format_expr(a), format_expr(b)),
        Expr::Mul(a, b) => format!("({} * {})", format_expr(a), format_expr(b)),
        Expr::Div(a, b) => format!("({} / {})", format_expr(a), format_expr(b)),
        Expr::Mod(a, b) => format!("({} % {})", format_expr(a), format_expr(b)),
        Expr::Gt(a, b) => format!("({} > {})", format_expr(a), format_expr(b)),
        Expr::Lt(a, b) => format!("({} < {})", format_expr(a), format_expr(b)),
        Expr::Eq(a, b) => format!("({} == {})", format_expr(a), format_expr(b)),
        Expr::And(a, b) => format!("({} && {})", format_expr(a), format_expr(b)),
        Expr::Or(a, b) => format!("({} || {})", format_expr(a), format_expr(b)),
        Expr::Min(a, b) => format!("min({}, {})", format_expr(a), format_expr(b)),
        Expr::Max(a, b) => format!("max({}, {})", format_expr(a), format_expr(b)),
        Expr::Not(a) => format!("!{}", format_expr(a)),
        Expr::Neg(a) => format!("-{}", format_expr(a)),
        Expr::Abs(a) => format!("abs({})", format_expr(a)),
    }
}

fn format_tile_ref(tr: &TileRef) -> String {
    match tr {
        TileRef::Absolute(x, y) => format!("const[{}],const[{}]", x, y),
        TileRef::Relative(dx, dy) => format!("iter+({},{})", dx, dy),
        TileRef::Capital => "capital".to_string(),
        TileRef::IterTile => "iter".to_string(),
    }
}

fn format_action(action: &Action) -> String {
    match action {
        Action::Move { from, to, count } => {
            format!("MOVE {} troops from {} to {}",
                format_expr(count), format_tile_ref(from), format_tile_ref(to))
        }
        Action::Convert { city, count } => {
            format!("CONVERT {} pop at {}", format_expr(count), format_tile_ref(city))
        }
        Action::MoveCapital { city } => {
            format!("MOVE_CAPITAL to {}", format_tile_ref(city))
        }
        Action::Store { reg, value } => {
            format!("STORE {} in reg[{}]", format_expr(value), reg)
        }
        Action::Repeat { count, inner } => {
            format!("REPEAT {} times: {}", format_expr(count), format_action(inner))
        }
        Action::Skip => "SKIP".to_string(),
    }
}
