use fto_core::{
    moves::Move,
    pruning::SolverPruning,
    search::{self, SearchConfig},
    tables::TransitionTables,
    FtoCubie,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

const U: usize = 0;
const F: usize = 9;
const R_LOWER: usize = 18;
const L_LOWER: usize = 27;
const D: usize = 36;
const B: usize = 45;
const R: usize = 54;
const L: usize = 63;

const CORNER_FACELETS: [[usize; 4]; 6] = [
    [U, R, F, L],
    [U + 4, B + 8, R_LOWER + 4, R + 8],
    [U + 8, L + 4, L_LOWER + 8, B + 4],
    [L_LOWER, D, R_LOWER, B],
    [F + 4, D + 8, L_LOWER + 4, L + 8],
    [R_LOWER + 8, D + 4, F + 8, R + 4],
];

const EDGE_FACELETS: [[usize; 2]; 12] = [
    [U + 1, R + 3],
    [U + 3, L + 1],
    [U + 6, B + 6],
    [L_LOWER + 1, D + 3],
    [R_LOWER + 3, D + 1],
    [F + 6, D + 6],
    [F + 3, R + 1],
    [F + 1, L + 3],
    [L_LOWER + 6, L + 6],
    [L_LOWER + 3, B + 1],
    [R_LOWER + 1, B + 3],
    [R_LOWER + 6, R + 6],
];

const UF_CENTER_FACELETS: [usize; 12] = [
    U + 2,
    U + 5,
    U + 7,
    F + 2,
    F + 5,
    F + 7,
    R_LOWER + 2,
    R_LOWER + 5,
    R_LOWER + 7,
    L_LOWER + 2,
    L_LOWER + 5,
    L_LOWER + 7,
];

const RL_CENTER_FACELETS: [usize; 12] = [
    D + 2,
    D + 5,
    D + 7,
    B + 2,
    B + 5,
    B + 7,
    L + 2,
    L + 5,
    L + 7,
    R + 2,
    R + 5,
    R + 7,
];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolveRequest {
    scramble: Option<String>,
    state: Option<CubieState>,
    facelets: Option<Vec<u8>>,
    allowed_moves: Vec<String>,
    max_depth: u8,
    exact: bool,
    find_all: bool,
    restricted_pruning: bool,
    threads: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CubieState {
    cp: [u8; 6],
    co: [u8; 6],
    ep: [u8; 12],
    uf: [u8; 12],
    rl: [u8; 12],
}

#[derive(Clone, Debug, Serialize)]
struct SolveResponse {
    nodes: u64,
    solutions: Vec<String>,
}

#[derive(Default)]
struct SolverState {
    current_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

#[tauri::command]
fn solve_fto(request: SolveRequest, solver_state: tauri::State<'_, SolverState>) -> Result<SolveResponse, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut current = solver_state
            .current_cancel
            .lock()
            .map_err(|_| "solver state lock poisoned".to_owned())?;
        if current.is_some() {
            return Err("a solve is already running".to_owned());
        }
        *current = Some(cancel.clone());
    }

    let result = solve_fto_inner(request, cancel);

    if let Ok(mut current) = solver_state.current_cancel.lock() {
        *current = None;
    }

    result
}

#[tauri::command]
fn stop_solve(solver_state: tauri::State<'_, SolverState>) -> Result<(), String> {
    let current = solver_state
        .current_cancel
        .lock()
        .map_err(|_| "solver state lock poisoned".to_owned())?;
    if let Some(cancel) = current.as_ref() {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
fn validate_facelets(facelets: Vec<u8>) -> Result<CubieState, String> {
    cubie_from_facelets(&facelets).map(cubie_to_state)
}

fn solve_fto_inner(request: SolveRequest, cancel: Arc<AtomicBool>) -> Result<SolveResponse, String> {
    let tables = TransitionTables::load_or_build("cache/transition-tables-v4.bin")
        .map_err(|error| error.to_string())?;
    let allowed_moves = parse_move_names(&request.allowed_moves)?;
    if allowed_moves.is_empty() {
        return Err("at least one move must be allowed".to_owned());
    }

    let cubie = if let Some(state) = request.state {
        FtoCubie::new(state.cp, state.co, state.ep, state.uf, state.rl)
    } else if let Some(facelets) = request.facelets {
        cubie_from_facelets(&facelets)?
    } else if let Some(scramble) = request.scramble {
        apply_sequence(FtoCubie::solved(), &scramble)?
    } else {
        FtoCubie::solved()
    };

    let pruning_moves = if request.restricted_pruning {
        allowed_moves.as_slice()
    } else {
        Move::ALL.as_slice()
    };
    let pruning =
        SolverPruning::load_or_build_with_moves(&tables, Path::new("cache/pruning-v4"), 5_000_000, pruning_moves)?;
    let result = search::solve_with_pruning_threads(
        cubie.coord(),
        &tables,
        Some(&pruning),
        &SearchConfig {
            min_depth: if request.exact { request.max_depth } else { 0 },
            max_depth: request.max_depth,
            find_all: request.find_all,
            allowed_moves,
            cancel: Some(cancel),
        },
        request.threads.max(1),
    );

    Ok(SolveResponse {
        nodes: result.nodes,
        solutions: result
            .solutions
            .iter()
            .map(|solution| search::format_solution(solution))
            .collect(),
    })
}

pub fn run() {
    tauri::Builder::default()
        .manage(SolverState::default())
        .invoke_handler(tauri::generate_handler![solve_fto, stop_solve, validate_facelets])
        .run(tauri::generate_context!())
        .expect("failed to run Tauri app");
}

fn cubie_to_state(cubie: FtoCubie) -> CubieState {
    CubieState {
        cp: cubie.cp,
        co: cubie.co,
        ep: cubie.ep,
        uf: cubie.uf,
        rl: cubie.rl,
    }
}

fn apply_sequence(mut cubie: FtoCubie, sequence: &str) -> Result<FtoCubie, String> {
    for token in sequence.split_whitespace() {
        cubie = cubie.apply(parse_move(token)?);
    }
    Ok(cubie)
}

fn cubie_from_facelets(facelets: &[u8]) -> Result<FtoCubie, String> {
    if facelets.len() != 72 {
        return Err("facelet input must contain exactly 72 stickers".to_owned());
    }

    let mut counts = [0_u8; 8];
    for &color in facelets {
        if color >= 8 {
            return Err(format!("facelet color {color} is outside 0..7"));
        }
        counts[color as usize] += 1;
    }
    for (color, &count) in counts.iter().enumerate() {
        if count != 9 {
            return Err(format!("color {color} appears {count} times; expected 9"));
        }
    }

    let mut cp = [0_u8; 6];
    let mut corner_ori = [0_u8; 6];
    detect_pieces(&CORNER_FACELETS, facelets, &mut cp, &mut corner_ori)
        .map_err(|_| "corner stickers do not describe the six FTO corners".to_owned())?;

    let mut co = [0_u8; 6];
    let mut corner_xor = 0_u8;
    for i in 0..6 {
        co[i] = corner_ori[i] >> 1;
        corner_xor ^= co[i];
    }
    if corner_xor != 0 {
        return Err("corner orientation parity is invalid".to_owned());
    }

    let mut ep = [0_u8; 12];
    let mut edge_ori = [0_u8; 12];
    detect_pieces(&EDGE_FACELETS, facelets, &mut ep, &mut edge_ori)
        .map_err(|_| "edge stickers do not describe the twelve FTO edges".to_owned())?;

    if permutation_parity(&cp) != 0 {
        return Err("corner permutation parity is invalid".to_owned());
    }
    if permutation_parity(&ep) != 0 {
        return Err("edge permutation parity is invalid".to_owned());
    }

    let mut uf = read_uf_centers(facelets)?;
    let mut rl = read_rl_centers(facelets)?;
    if permutation_parity(&uf) != 0 {
        swap_zero_one(&mut uf);
    }
    if permutation_parity(&rl) != 0 {
        swap_zero_one(&mut rl);
    }

    Ok(FtoCubie::new(cp, co, ep, uf, rl))
}

fn detect_pieces<const N: usize, const K: usize>(
    piece_facelets: &[[usize; K]; N],
    colors: &[u8],
    perm: &mut [u8; N],
    ori: &mut [u8; N],
) -> Result<(), ()> {
    let mut used = [false; 12];
    for pos in 0..N {
        let mut matched = false;
        for piece in 0..N {
            if used[piece] {
                continue;
            }
            for orientation in 0..K {
                let mut ok = true;
                for t in 0..K {
                    let expected = (piece_facelets[piece][t] / 9) as u8;
                    let actual = colors[piece_facelets[pos][(t + orientation) % K]];
                    if expected != actual {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    perm[pos] = piece as u8;
                    ori[pos] = orientation as u8;
                    used[piece] = true;
                    matched = true;
                    break;
                }
            }
            if matched {
                break;
            }
        }
        if !matched {
            return Err(());
        }
    }
    Ok(())
}

fn read_uf_centers(facelets: &[u8]) -> Result<[u8; 12], String> {
    let mut remain = [3_u8; 4];
    let mut out = [0_u8; 12];
    for (i, &facelet) in UF_CENTER_FACELETS.iter().enumerate() {
        let color = facelets[facelet] as usize;
        if color >= 4 || remain[color] == 0 {
            return Err("UF center orbit has invalid color counts".to_owned());
        }
        out[i] = color as u8 * 3 + 3 - remain[color];
        remain[color] -= 1;
    }
    Ok(out)
}

fn read_rl_centers(facelets: &[u8]) -> Result<[u8; 12], String> {
    let mut remain = [3_u8; 4];
    let mut out = [0_u8; 12];
    for (i, &facelet) in RL_CENTER_FACELETS.iter().enumerate() {
        let color = facelets[facelet];
        if !(4..8).contains(&color) {
            return Err("RL center orbit has invalid colors".to_owned());
        }
        let mapped = [0_usize, 1, 3, 2][usize::from(color - 4)];
        if remain[mapped] == 0 {
            return Err("RL center orbit has invalid color counts".to_owned());
        }
        out[i] = mapped as u8 * 3 + 3 - remain[mapped];
        remain[mapped] -= 1;
    }
    Ok(out)
}

fn permutation_parity<const N: usize>(perm: &[u8; N]) -> u8 {
    let mut parity = 0_u8;
    for i in 0..N {
        for j in i + 1..N {
            parity ^= u8::from(perm[i] > perm[j]);
        }
    }
    parity
}

fn swap_zero_one<const N: usize>(perm: &mut [u8; N]) {
    for value in perm {
        if *value < 2 {
            *value ^= 1;
        }
    }
}

fn parse_move_names(names: &[String]) -> Result<Vec<Move>, String> {
    names.iter().map(|name| parse_move(name)).collect()
}

fn parse_move(token: &str) -> Result<Move, String> {
    match token {
        "U" => Ok(Move::U),
        "U'" | "Ui" => Ok(Move::Up),
        "F" => Ok(Move::F),
        "F'" | "Fi" => Ok(Move::Fp),
        "BR" | "r" => Ok(Move::Br),
        "BR'" | "BRi" | "r'" | "ri" => Ok(Move::Brp),
        "BL" | "l" => Ok(Move::Bl),
        "BL'" | "BLi" | "l'" | "li" => Ok(Move::Blp),
        "D" => Ok(Move::D),
        "D'" | "Di" => Ok(Move::Dp),
        "B" => Ok(Move::B),
        "B'" | "Bi" => Ok(Move::Bp),
        "R" => Ok(Move::R),
        "R'" | "Ri" => Ok(Move::Rp),
        "L" => Ok(Move::L),
        "L'" | "Li" => Ok(Move::Lp),
        "Uw" => Ok(Move::Uw),
        "Uw'" | "Uwi" => Ok(Move::Uwp),
        "Fw" => Ok(Move::Fw),
        "Fw'" | "Fwi" => Ok(Move::Fwp),
        "Rw" => Ok(Move::Rw),
        "Rw'" | "Rwi" => Ok(Move::Rwp),
        "Lw" => Ok(Move::Lw),
        "Lw'" | "Lwi" => Ok(Move::Lwp),
        "M" => Ok(Move::M),
        "M'" | "Mi" => Ok(Move::Mp),
        _ => Err(format!("unknown move: {token}")),
    }
}
