use fto_core::{
    moves::Move,
    partial::{self, PartialMask, PartialProblem},
    pruning::{self, SolverPruning},
    search::{self, MiniPruning, SearchConfig},
    tables::TransitionTables,
    FtoCubie,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter};

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
const RL_LAST_LAYER_FIXED_CENTER_SLOT: [Option<u8>; 4] = [None, Some(3), Some(8), Some(10)];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolveRequest {
    scramble: Option<String>,
    state: Option<CubieState>,
    facelets: Option<Vec<u8>>,
    center_targets: Option<CenterTargets>,
    allowed_moves: Vec<String>,
    instance_moves: Vec<String>,
    max_depth: Option<u8>,
    find_all: bool,
    restricted_pruning: bool,
    mini_pruning: bool,
    last_layer_mode: bool,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CenterTargets {
    uf: [Option<u8>; 4],
    rl: [Option<u8>; 4],
    uf_sources: [Option<u8>; 4],
    rl_sources: [Option<u8>; 4],
    #[serde(default = "empty_center_source_groups")]
    rl_top_sources: [Vec<u8>; 4],
}

fn empty_center_source_groups() -> [Vec<u8>; 4] {
    [Vec::new(), Vec::new(), Vec::new(), Vec::new()]
}

#[derive(Clone, Debug, Serialize)]
struct SolveResponse {
    nodes: u64,
    solutions: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct SolveLine {
    kind: String,
    text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PruningCacheStatus {
    has_tables: bool,
    has_pruning: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PruningTableInfo {
    id: String,
    codename: String,
    moves: Vec<String>,
    files: Vec<String>,
    bytes: u64,
}

#[derive(Default)]
struct SolverState {
    current_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    cache: Arc<Mutex<SolverCache>>,
}

#[derive(Default)]
struct SolverCache {
    tables: Option<Arc<TransitionTables>>,
    pruning: Option<CachedPruning>,
}

struct CachedPruning {
    moves: Vec<Move>,
    pruning: Arc<SolverPruning>,
}

#[tauri::command]
async fn solve_fto(
    request: SolveRequest,
    solver_state: tauri::State<'_, SolverState>,
    app: AppHandle,
) -> Result<(), String> {
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

    let cancels = solver_state.current_cancel.clone();
    let cache = solver_state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let response = run_solve_blocking(&request, &cancel, &app, &cancels, &cache);
        let _ = response;
    });

    Ok(())
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
fn unload_pruning_table(solver_state: tauri::State<'_, SolverState>) -> Result<(), String> {
    let mut cache = solver_state
        .cache
        .lock()
        .map_err(|_| "solver cache lock poisoned".to_owned())?;
    cache.tables = None;
    cache.pruning = None;
    Ok(())
}

#[tauri::command]
fn pruning_cache_status(solver_state: tauri::State<'_, SolverState>) -> Result<PruningCacheStatus, String> {
    let cache = solver_state
        .cache
        .lock()
        .map_err(|_| "solver cache lock poisoned".to_owned())?;
    Ok(PruningCacheStatus {
        has_tables: cache.tables.is_some(),
        has_pruning: cache.pruning.is_some(),
    })
}

#[tauri::command]
fn list_pruning_tables() -> Result<Vec<PruningTableInfo>, String> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")));
    let pruning_dir = workspace_root.join("cache/pruning-v4");
    discover_pruning_tables(&pruning_dir)
}

#[tauri::command]
fn delete_pruning_table(id: String, solver_state: tauri::State<'_, SolverState>) -> Result<(), String> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")));
    let pruning_dir = workspace_root.join("cache/pruning-v4");
    let tables = discover_pruning_tables(&pruning_dir)?;
    let table = tables
        .into_iter()
        .find(|table| table.id == id)
        .ok_or_else(|| "pruning table not found".to_owned())?;
    for file in table.files {
        let path = pruning_dir.join(file);
        if path.exists() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    unload_pruning_table(solver_state)
}

#[tauri::command]
fn validate_facelets(facelets: Vec<u8>) -> Result<CubieState, String> {
    cubie_from_facelets_for_solving(&facelets).map(cubie_to_state)
}

fn run_solve_blocking(
    request: &SolveRequest,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
    cancels: &Mutex<Option<Arc<AtomicBool>>>,
    cache: &Mutex<SolverCache>,
) -> () {
    let result = solve_fto_inner(request, cancel, app, cache);

    if let Ok(mut current) = cancels.lock() {
        *current = None;
    }

    if cancel.load(Ordering::Relaxed) {
        emit_line(app, "cancel", "solve cancelled");
        let _ = app.emit("solve-cancelled", ());
        return;
    }

    match result {
        Ok(response) => {
            let _ = app.emit("solve-result", response);
        }
        Err(error) => {
            emit_line(app, "error", &format!("error: {error}"));
            let _ = app.emit("solve-error", error);
        }
    }
}

fn emit_line(app: &AppHandle, kind: &str, text: &str) {
    let _ = app.emit(
        "solve-line",
        SolveLine {
            kind: kind.to_owned(),
            text: text.to_owned(),
        },
    );
}

fn solve_fto_inner(
    request: &SolveRequest,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
    cache: &Mutex<SolverCache>,
) -> Result<SolveResponse, String> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")));

    let allowed_moves = parse_move_names(&request.allowed_moves)?;
    if allowed_moves.is_empty() {
        return Err("at least one move must be allowed".to_owned());
    }
    let instance_moves = if request.instance_moves.is_empty() {
        Move::ALL.to_vec()
    } else {
        parse_move_names(&request.instance_moves)?
    };
    let instance_move_set = instance_moves.iter().copied().collect::<std::collections::HashSet<_>>();
    if !allowed_moves.iter().all(|mv| instance_move_set.contains(mv)) {
        return Err(
            "the selected move set is not contained in this instance's move set".to_owned(),
        );
    }

    let (cubie, partial_mask) = if let Some(state) = request.state.clone() {
        (FtoCubie::new(state.cp, state.co, state.ep, state.uf, state.rl), None)
    } else if let Some(facelets) = request.facelets.clone() {
        cubie_and_partial_mask_from_facelets(
            &facelets,
            request.center_targets.as_ref(),
            request.last_layer_mode,
        )?
    } else if let Some(scramble) = request.scramble.clone() {
        (apply_sequence(FtoCubie::solved(), &scramble)?, None)
    } else {
        (FtoCubie::solved(), None)
    };

    run_in_process_solve(
        workspace_root,
        request,
        allowed_moves,
        instance_moves,
        cubie,
        partial_mask,
        cancel,
        app,
        cache,
    )
}

pub fn run() {
    tauri::Builder::default()
        .manage(SolverState::default())
        .invoke_handler(tauri::generate_handler![
            solve_fto,
            stop_solve,
            unload_pruning_table,
            pruning_cache_status,
            list_pruning_tables,
            delete_pruning_table,
            validate_facelets
        ])
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

fn run_in_process_solve(
    workspace_root: &Path,
    request: &SolveRequest,
    allowed_moves: Vec<Move>,
    instance_moves: Vec<Move>,
    cubie: FtoCubie,
    partial_mask: Option<PartialMask>,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
    cache: &Mutex<SolverCache>,
) -> Result<SolveResponse, String> {
    let tables_path = workspace_root.join("cache/transition-tables-v4.bin");
    let pruning_dir = workspace_root.join("cache/pruning-v4");
    emit_line(app, "info", "loading pruning tables...");
    let tables = load_transition_tables(cache, &tables_path)?;

    if let Some(mask) = partial_mask {
        let problem = PartialProblem { cubie, mask };
        let reporter = solution_reporter(app.clone());
        let config = SearchConfig {
            min_depth: request.max_depth.unwrap_or(0),
            max_depth: request.max_depth.unwrap_or(u8::MAX),
            find_all: request.find_all,
            allowed_moves: allowed_moves.clone(),
            free_u_ends: request.last_layer_mode,
            cancel: Some(cancel.clone()),
            solution_reporter: Some(reporter),
        };
        emit_line(app, "info", "using dynamic partial pruning tables");
        let result = partial::solve_partial_threads(&problem, &config, request.threads.max(1));
        let solutions = format_solutions(&result.solutions);
        emit_solve_summary(app, result.nodes, &solutions);
        return Ok(SolveResponse { nodes: result.nodes, solutions });
    }

    let pruning = Some(load_solver_pruning(
        cache,
        &tables,
        &pruning_dir,
        250_000,
        &allowed_moves,
        &instance_moves,
        request.restricted_pruning,
        cancel,
        app,
    )?);
    let mini = if request.mini_pruning && !request.last_layer_mode {
        let start = Instant::now();
        let mini = MiniPruning::build_restricted(&tables, &allowed_moves);
        emit_line(
            app,
            "info",
            &format!(
                "using {} restricted mini pruning tables ({:.1} MiB, built in {:.3}s)",
                mini.len(),
                mini.bytes() as f64 / (1024.0 * 1024.0),
                start.elapsed().as_secs_f64()
            ),
        );
        Some(mini)
    } else {
        None
    };

    let result = if let Some(max_depth) = request.max_depth {
        solve_once(
            cubie,
            &tables,
            pruning.as_deref(),
            mini.as_ref(),
            max_depth,
            true,
            request.find_all,
            request.threads.max(1),
            allowed_moves.clone(),
            request.last_layer_mode,
            cancel,
            app,
        )?
    } else {
        solve_incrementally(
            cubie,
            &tables,
            pruning.as_deref(),
            mini.as_ref(),
            request.find_all,
            request.threads.max(1),
            allowed_moves.clone(),
            request.last_layer_mode,
            cancel,
            app,
        )?
    };
    let solutions = format_solutions(&result.solutions);
    emit_solve_summary(app, result.nodes, &solutions);
    Ok(SolveResponse { nodes: result.nodes, solutions })
}

fn load_transition_tables(
    cache: &Mutex<SolverCache>,
    path: &Path,
) -> Result<Arc<TransitionTables>, String> {
    let mut cache = cache
        .lock()
        .map_err(|_| "solver cache lock poisoned".to_owned())?;
    if let Some(tables) = cache.tables.as_ref() {
        return Ok(tables.clone());
    }
    let tables = Arc::new(TransitionTables::load_or_build(path).map_err(|error| error.to_string())?);
    cache.tables = Some(tables.clone());
    Ok(tables)
}

fn load_solver_pruning(
    cache: &Mutex<SolverCache>,
    tables: &TransitionTables,
    out_dir: &Path,
    progress_interval: usize,
    allowed_moves: &[Move],
    instance_moves: &[Move],
    restricted_pruning: bool,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
) -> Result<Arc<SolverPruning>, String> {
    let target = select_pruning_move_set(out_dir, allowed_moves, instance_moves, restricted_pruning)?;
    {
        let cache = cache
            .lock()
            .map_err(|_| "solver cache lock poisoned".to_owned())?;
        if let Some(cached) = cache.pruning.as_ref().filter(|cached| cached.moves == target) {
            emit_line(app, "info", "using cached pruning tables from RAM");
            return Ok(cached.pruning.clone());
        }
    }

    let progress_app = app.clone();
    let progress = move |event: pruning::PruningProgress| {
        let percent = if event.total == 0 {
            0.0
        } else {
            (event.reached as f64 * 100.0) / event.total as f64
        };
        emit_line(
            &progress_app,
            "progress",
            &format!(
                "pruning progress: {} reached {}/{} ({percent:.1}%) depth {} expanded {}",
                event.name, event.reached, event.total, event.depth, event.expanded
            ),
        );
    };
    let pruning = Arc::new(
        SolverPruning::load_or_build_with_moves_reporting(
            tables,
            out_dir,
            progress_interval,
            &target,
            Some(cancel),
            Some(&progress),
        )
        .map_err(|error| error.to_string())?,
    );
    emit_line(
        app,
        "info",
        &format!(
            "using 2 pruning tables: edge3+uf3 / corner+uf3 built with moves: {} ({:.1} MiB)",
            format_move_list(&target),
            pruning.total_bytes() as f64 / (1024.0 * 1024.0)
        ),
    );
    let mut cache = cache
        .lock()
        .map_err(|_| "solver cache lock poisoned".to_owned())?;
    cache.pruning = Some(CachedPruning {
        moves: target,
        pruning: pruning.clone(),
    });
    Ok(pruning)
}

fn solve_once(
    cubie: FtoCubie,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    mini: Option<&MiniPruning>,
    max_depth: u8,
    exact_depth: bool,
    find_all: bool,
    threads: usize,
    allowed_moves: Vec<Move>,
    last_layer_mode: bool,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
) -> Result<search::SearchResult, String> {
    emit_line(app, "search", &format!("searching depth {max_depth}..."));
    let reporter = solution_reporter(app.clone());
    let config = SearchConfig {
        min_depth: if exact_depth { max_depth } else { 0 },
        max_depth,
        find_all,
        allowed_moves,
        free_u_ends: last_layer_mode,
        cancel: Some(cancel.clone()),
        solution_reporter: Some(reporter),
    };
    let result = if last_layer_mode {
        search::solve_last_layer_with_pruning_threads(cubie, tables, pruning, &config, threads)
    } else {
        search::solve_with_pruning_and_mini_threads(
            cubie.coord(),
            tables,
            pruning,
            mini,
            &config,
            threads,
        )
    };
    if !result.solutions.is_empty() {
        emit_line(app, "info", &format!("found solution at depth {max_depth}"));
    }
    Ok(result)
}

fn solve_incrementally(
    cubie: FtoCubie,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    mini: Option<&MiniPruning>,
    find_all: bool,
    threads: usize,
    allowed_moves: Vec<Move>,
    last_layer_mode: bool,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
) -> Result<search::SearchResult, String> {
    let coord = cubie.coord();
    let start_depth = if last_layer_mode {
        0
    } else {
        pruning
            .map(|tables| tables.heuristic_for_coord(coord))
            .unwrap_or(0)
    };
    let mut total_nodes = 0_u64;
    for depth in start_depth..=u8::MAX {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        emit_line(app, "search", &format!("searching depth {depth}..."));
        let reporter = solution_reporter(app.clone());
        let config = SearchConfig {
            min_depth: depth,
            max_depth: depth,
            find_all,
            allowed_moves: allowed_moves.clone(),
            free_u_ends: last_layer_mode,
            cancel: Some(cancel.clone()),
            solution_reporter: Some(reporter),
        };
        let mut result = if last_layer_mode {
            search::solve_last_layer_with_pruning_threads(cubie, tables, pruning, &config, threads)
        } else {
            search::solve_with_pruning_and_mini_threads(
                coord,
                tables,
                pruning,
                mini,
                &config,
                threads,
            )
        };
        total_nodes += result.nodes;
        if !result.solutions.is_empty() {
            result.nodes = total_nodes;
            emit_line(app, "info", &format!("found solution at depth {depth}"));
            return Ok(result);
        }
    }
    Err("no solution found up to depth 255".to_owned())
}

fn emit_solve_summary(app: &AppHandle, nodes: u64, solutions: &[String]) {
    emit_line(app, "done", &format!("nodes: {nodes}"));
    emit_line(app, "done", &format!("solutions: {}", solutions.len()));
    for solution in solutions {
        emit_line(app, "solution", solution);
    }
}

fn solution_reporter(app: AppHandle) -> Arc<search::SolutionReporter> {
    Arc::new(move |solution| {
        emit_line(&app, "solution", &search::format_solution(solution));
    })
}

fn format_solutions(solutions: &[Vec<Move>]) -> Vec<String> {
    solutions
        .iter()
        .map(|solution| search::format_solution(solution))
        .collect()
}

fn discover_pruning_tables(out_dir: &Path) -> Result<Vec<PruningTableInfo>, String> {
    let mut tables = Vec::new();
    let entries = match fs::read_dir(out_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(tables),
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(file_suffix_ref) = name
            .strip_prefix("edge3__uf3__")
            .and_then(|name| name.strip_suffix(".pdb"))
        else {
            continue;
        };
        let file_suffix = file_suffix_ref.to_owned();
        let suffix = read_move_suffix(out_dir, &file_suffix).unwrap_or_else(|| file_suffix.to_owned());
        let corner = format!("corner__uf3__{file_suffix}.pdb");
        if !out_dir.join(&corner).exists() {
            continue;
        }
        let Some(mut moves) = pruning::parse_move_set_suffix(&suffix) else {
            continue;
        };
        moves.sort_unstable_by_key(|mv| mv.idx());
        let mut files = vec![name, corner];
        let edge_meta = format!("edge3__uf3__{file_suffix}.moves");
        let corner_meta = format!("corner__uf3__{file_suffix}.moves");
        if out_dir.join(&edge_meta).exists() {
            files.push(edge_meta);
        }
        if out_dir.join(&corner_meta).exists() {
            files.push(corner_meta);
        }
        let bytes = files.iter().try_fold(0_u64, |total, file| {
            fs::metadata(out_dir.join(file))
                .map(|metadata| total + metadata.len())
                .map_err(|error| error.to_string())
        })?;
        tables.push(PruningTableInfo {
            id: file_suffix.clone(),
            codename: pruning_codename(&file_suffix),
            moves: moves.iter().map(|mv| format!("{mv:?}")).collect(),
            files,
            bytes,
        });
    }
    tables.sort_by(|a, b| a.codename.cmp(&b.codename));
    Ok(tables)
}

fn read_move_suffix(out_dir: &Path, file_suffix: &str) -> Option<String> {
    fs::read_to_string(out_dir.join(format!("edge3__uf3__{file_suffix}.moves")))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn pruning_codename(suffix: &str) -> String {
    use std::hash::{Hash, Hasher};

    const NAMES: [&str; 16] = [
        "Aster", "Boreal", "Cipher", "Drift", "Ember", "Fable", "Glint", "Halo",
        "Ivory", "Jade", "Kestrel", "Lumen", "Morrow", "Nimbus", "Oracle", "Vesper",
    ];
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    suffix.hash(&mut hasher);
    let value = hasher.finish();
    format!("{}-{:04X}", NAMES[value as usize % NAMES.len()], value as u16)
}

fn select_pruning_move_set(
    out_dir: &Path,
    allowed_moves: &[Move],
    instance_moves: &[Move],
    restricted_pruning: bool,
) -> Result<Vec<Move>, String> {
    if allowed_moves.is_empty() {
        return Err("at least one move must be allowed".to_owned());
    }
    let instance_set: HashSet<Move> = HashSet::from_iter(instance_moves.iter().copied());
    if !allowed_moves.iter().all(|mv| instance_set.contains(mv)) {
        return Err("the selected move set is not contained in this instance's move set".to_owned());
    }

    let mut candidates = Vec::<Vec<Move>>::new();
    candidates.extend(discover_cached_move_sets(out_dir)?);
    if restricted_pruning {
        candidates.push(allowed_moves.to_vec());
    }
    candidates.push(instance_moves.to_vec());
    if instance_moves != Move::ALL.as_slice() {
        candidates.push(Move::ALL.to_vec());
    }

    let mut allowed_sorted = allowed_moves.to_vec();
    allowed_sorted.sort_unstable_by_key(|mv| mv.idx());

    let mut best: Option<&Vec<Move>> = None;
    for candidate in &candidates {
        let set: HashSet<Move> = HashSet::from_iter(candidate.iter().copied());
        if !allowed_moves.iter().all(|mv| set.contains(mv)) {
            continue;
        }
        let better = match best {
            Some(current) => {
                candidate.len() < current.len()
                    || (candidate.len() == current.len()
                        && candidate == &allowed_sorted
                        && current != &allowed_sorted)
            }
            None => true,
        };
        if better {
            best = Some(candidate);
        }
    }

    best.cloned()
        .ok_or_else(|| "no cached pruning table move set covers the solve move set".to_owned())
}

fn discover_cached_move_sets(out_dir: &Path) -> Result<Vec<Vec<Move>>, String> {
    let mut sets = Vec::<Vec<Move>>::new();
    let entries = match fs::read_dir(out_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(sets),
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(file_suffix) = name
            .strip_prefix("edge3__uf3__")
            .and_then(|name| name.strip_suffix(".pdb"))
        else {
            continue;
        };
        if !out_dir.join(format!("corner__uf3__{file_suffix}.pdb")).exists() {
            continue;
        }
        let suffix = read_move_suffix(out_dir, file_suffix).unwrap_or_else(|| file_suffix.to_owned());
        if let Some(mut moves) = pruning::parse_move_set_suffix(&suffix) {
            moves.sort_unstable_by_key(|mv| mv.idx());
            if !sets.contains(&moves) {
                sets.push(moves);
            }
        }
    }
    Ok(sets)
}

fn format_move_list(moves: &[Move]) -> String {
    moves
        .iter()
        .map(|mv| format!("{mv:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn cubie_from_facelets_for_solving(facelets: &[u8]) -> Result<FtoCubie, String> {
    cubie_and_partial_mask_from_facelets(facelets, None, false).map(|(cubie, _)| cubie)
}

fn cubie_and_partial_mask_from_facelets(
    facelets: &[u8],
    center_targets: Option<&CenterTargets>,
    last_layer_mode: bool,
) -> Result<(FtoCubie, Option<PartialMask>), String> {
    if facelets.iter().any(|&color| color == 8) {
        cubie_from_partial_facelets(facelets, center_targets, last_layer_mode)
            .map(|(cubie, mask)| (cubie, Some(mask)))
    } else if last_layer_mode {
        cubie_from_facelets_with_center_targets(facelets, center_targets).map(|cubie| (cubie, None))
    } else {
        cubie_from_facelets(facelets).map(|cubie| (cubie, None))
    }
}

fn cubie_from_partial_facelets(
    facelets: &[u8],
    center_targets: Option<&CenterTargets>,
    last_layer_mode: bool,
) -> Result<(FtoCubie, PartialMask), String> {
    if facelets.len() != 72 {
        return Err("facelet input must contain exactly 72 stickers".to_owned());
    }
    for &color in facelets {
        if color > 8 {
            return Err(format!("facelet color {color} is outside 0..8"));
        }
    }
    let slot_mask = partial_slot_mask_from_facelets(facelets)?;
    let (mut cp, corner_ori) = detect_partial_pieces(&CORNER_FACELETS, facelets, &slot_mask.corners)
        .map_err(|_| "defined corner stickers do not describe legal FTO corners".to_owned())?;
    let mut co = [0_u8; 6];
    let mut corner_xor = 0_u8;
    for i in 0..6 {
        co[i] = corner_ori[i] >> 1;
        corner_xor ^= co[i];
    }
    if corner_xor != 0 {
        let ignored_corner = slot_mask
            .corners
            .iter()
            .position(|&care| !care)
            .ok_or_else(|| "defined corner orientation parity is invalid".to_owned())?;
        co[ignored_corner] ^= corner_xor;
    }

    let (mut ep, _) = detect_partial_pieces(&EDGE_FACELETS, facelets, &slot_mask.edges)
        .map_err(|_| "defined edge stickers do not describe legal FTO edges".to_owned())?;
    repair_partial_parity(&mut cp, &slot_mask.corners)?;
    repair_partial_parity(&mut ep, &slot_mask.edges)?;

    let uf = read_partial_centers(
        facelets,
        &UF_CENTER_FACELETS,
        &slot_mask.uf_centers,
        0,
        center_targets.map(|targets| &targets.uf),
        center_targets.map(|targets| &targets.uf_sources),
        None,
        None,
    )?;
    let rl = read_partial_centers(
        facelets,
        &RL_CENTER_FACELETS,
        &slot_mask.rl_centers,
        4,
        center_targets.map(|targets| &targets.rl),
        center_targets.map(|targets| &targets.rl_sources),
        center_targets.map(|targets| &targets.rl_top_sources),
        Some(&RL_LAST_LAYER_FIXED_CENTER_SLOT),
    )?;
    let mut mask = identity_mask_from_slot_mask(&slot_mask, &cp, &ep, &uf, &rl);
    mask.last_layer_centers = last_layer_mode;
    apply_center_targets(&mut mask, center_targets)?;

    Ok((FtoCubie::new(cp, co, ep, uf, rl), mask))
}

fn partial_slot_mask_from_facelets(facelets: &[u8]) -> Result<PartialMask, String> {
    if facelets.len() != 72 {
        return Err("facelet input must contain exactly 72 stickers".to_owned());
    }
    Ok(PartialMask {
        corners: care_mask(&CORNER_FACELETS, facelets),
        edges: care_mask(&EDGE_FACELETS, facelets),
        uf_centers: center_care_mask(&UF_CENTER_FACELETS, facelets),
        rl_centers: center_care_mask(&RL_CENTER_FACELETS, facelets),
        uf_center_targets: [None; 4],
        rl_center_targets: [None; 4],
        last_layer_centers: false,
    })
}

fn identity_mask_from_slot_mask(
    slot_mask: &PartialMask,
    cp: &[u8; 6],
    ep: &[u8; 12],
    uf: &[u8; 12],
    rl: &[u8; 12],
) -> PartialMask {
    let mut mask = PartialMask {
        corners: [false; 6],
        edges: [false; 12],
        uf_centers: [false; 12],
        rl_centers: [false; 12],
        uf_center_targets: [None; 4],
        rl_center_targets: [None; 4],
        last_layer_centers: false,
    };
    for pos in 0..6 {
        if slot_mask.corners[pos] {
            mask.corners[cp[pos] as usize] = true;
        }
    }
    for pos in 0..12 {
        if slot_mask.edges[pos] {
            mask.edges[ep[pos] as usize] = true;
        }
        if slot_mask.uf_centers[pos] {
            mask.uf_centers[uf[pos] as usize] = true;
        }
        if slot_mask.rl_centers[pos] {
            mask.rl_centers[rl[pos] as usize] = true;
        }
    }
    mask
}

fn apply_center_targets(
    mask: &mut PartialMask,
    center_targets: Option<&CenterTargets>,
) -> Result<(), String> {
    let Some(center_targets) = center_targets else {
        return Ok(());
    };
    apply_center_target_orbit(
        "UF",
        &mask.uf_centers,
        &center_targets.uf,
        &mut mask.uf_center_targets,
    )?;
    apply_center_target_orbit(
        "RL",
        &mask.rl_centers,
        &center_targets.rl,
        &mut mask.rl_center_targets,
    )
}

fn apply_center_target_orbit(
    orbit_name: &str,
    cared_pieces: &[bool; 12],
    requested: &[Option<u8>; 4],
    out: &mut [Option<u8>; 4],
) -> Result<(), String> {
    for color in 0..4 {
        let Some(slot) = requested[color] else {
            continue;
        };
        if slot >= 12 {
            return Err(format!("{orbit_name} center target {slot} is outside 0..11"));
        }
        if slot / 3 != color as u8 {
            return Err(format!(
                "{orbit_name} center target {slot} does not match center color group {color}"
            ));
        }
        let cared_count = cared_pieces
            .iter()
            .enumerate()
            .filter(|(piece, care)| **care && piece / 3 == color)
            .count();
        if cared_count == 1 {
            out[color] = Some(slot);
        }
    }
    Ok(())
}

fn care_mask<const N: usize, const K: usize>(
    piece_facelets: &[[usize; K]; N],
    facelets: &[u8],
) -> [bool; N] {
    let mut mask = [true; N];
    for pos in 0..N {
        mask[pos] = piece_facelets[pos]
            .iter()
            .all(|&facelet| facelets[facelet] != 8);
    }
    mask
}

fn center_care_mask<const N: usize>(center_facelets: &[usize; N], facelets: &[u8]) -> [bool; N] {
    let mut mask = [true; N];
    for pos in 0..N {
        mask[pos] = facelets[center_facelets[pos]] != 8;
    }
    mask
}

fn detect_partial_pieces<const N: usize, const K: usize>(
    piece_facelets: &[[usize; K]; N],
    colors: &[u8],
    care: &[bool; N],
) -> Result<([u8; N], [u8; N]), ()> {
    let mut perm = [u8::MAX; N];
    let mut ori = [0_u8; N];
    let mut used = [false; 12];
    for pos in 0..N {
        if !care[pos] {
            continue;
        }
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

    let mut unused = (0..N)
        .filter(|&piece| !used[piece])
        .map(|piece| piece as u8)
        .collect::<Vec<_>>();
    for pos in 0..N {
        if perm[pos] == u8::MAX {
            perm[pos] = unused.pop().ok_or(())?;
        }
    }
    Ok((perm, ori))
}

fn repair_partial_parity<const N: usize>(perm: &mut [u8; N], care: &[bool; N]) -> Result<(), String> {
    if permutation_parity(perm) == 0 {
        return Ok(());
    }
    let ignored = care
        .iter()
        .enumerate()
        .filter_map(|(idx, &care)| (!care).then_some(idx))
        .collect::<Vec<_>>();
    if ignored.len() >= 2 {
        perm.swap(ignored[0], ignored[1]);
        Ok(())
    } else {
        Err("partial position has fixed permutation parity with no ignored pieces to absorb it".to_owned())
    }
}

fn read_partial_centers<const N: usize>(
    facelets: &[u8],
    center_facelets: &[usize; N],
    care: &[bool; N],
    color_base: u8,
    center_targets: Option<&[Option<u8>; 4]>,
    center_sources: Option<&[Option<u8>; 4]>,
    top_sources: Option<&[Vec<u8>; 4]>,
    fixed_slots: Option<&[Option<u8>; 4]>,
) -> Result<[u8; N], String> {
    let mut cared_per_color = [0_u8; 4];
    let mut visible_color = [usize::MAX; N];
    for pos in 0..N {
        if !care[pos] {
            continue;
        }
        let color = facelets[center_facelets[pos]];
        if color == 8 {
            continue;
        }
        if !(color_base..color_base + 4).contains(&color) {
            return Err("defined center orbit has invalid colors".to_owned());
        }
        let mapped = if color_base == 4 {
            [0_usize, 1, 3, 2][usize::from(color - 4)]
        } else {
            usize::from(color)
        };
        if cared_per_color[mapped] >= 3 {
            return Err("defined center orbit has too many centers of one color".to_owned());
        }
        visible_color[pos] = mapped;
        cared_per_color[mapped] += 1;
    }

    let mut used_piece = [false; N];
    let mut out = [u8::MAX; N];

    for color in 0..4 {
        let Some(target) = center_targets.and_then(|targets| targets[color]) else {
            continue;
        };
        if usize::from(target) >= N {
            return Err("defined center orbit target is outside the orbit".to_owned());
        }
        if target / 3 != color as u8 {
            return Err("defined center orbit target has the wrong color".to_owned());
        }
        let source = center_sources
            .and_then(|sources| sources[color])
            .and_then(|source| {
                (usize::from(source) < N
                    && care[usize::from(source)]
                    && visible_color[usize::from(source)] == color)
                    .then_some(usize::from(source))
            })
            .or_else(|| {
                (cared_per_color[color] == 1).then(|| {
                    (0..N)
                        .find(|&pos| care[pos] && visible_color[pos] == color)
                        .expect("one cared center of this color should exist")
                })
            });
        let Some(source) = source else {
            continue;
        };
        if used_piece[usize::from(target)] {
            return Err("partial center fill failed".to_owned());
        }
        out[source] = target;
        used_piece[usize::from(target)] = true;
    }

    if let Some(top_sources) = top_sources {
        for color in 0..4 {
            let fixed_slot = fixed_slots
                .and_then(|slots| slots[color])
                .map(usize::from);
            for &source in &top_sources[color] {
                let source = usize::from(source);
                if source >= N || !care[source] || visible_color[source] != color || out[source] != u8::MAX {
                    continue;
                }
                let piece = (color * 3..color * 3 + 3)
                    .find(|&piece| !used_piece[piece] && Some(piece) != fixed_slot)
                    .ok_or_else(|| "partial center fill failed".to_owned())?;
                out[source] = piece as u8;
                used_piece[piece] = true;
            }
        }
    }

    for pos in 0..N {
        if !care[pos] {
            continue;
        }
        let mapped = visible_color[pos];
        if mapped == usize::MAX {
            continue;
        }
        if out[pos] != u8::MAX {
            continue;
        }
        let piece = (mapped * 3..mapped * 3 + 3)
            .find(|&piece| !used_piece[piece])
            .ok_or_else(|| "partial center fill failed".to_owned())?;
        out[pos] = piece as u8;
        used_piece[piece] = true;
    }
    for pos in 0..N {
        if out[pos] != u8::MAX {
            used_piece[out[pos] as usize] = true;
        }
    }
    for pos in 0..N {
        if out[pos] != u8::MAX {
            continue;
        }
        let piece = (0..N)
            .find(|&piece| !used_piece[piece])
            .ok_or_else(|| "partial center fill failed".to_owned())?;
        out[pos] = piece as u8;
        used_piece[piece] = true;
    }
    Ok(out)
}

fn cubie_from_facelets(facelets: &[u8]) -> Result<FtoCubie, String> {
    cubie_from_facelets_with_optional_center_targets(facelets, None)
}

fn cubie_from_facelets_with_center_targets(
    facelets: &[u8],
    center_targets: Option<&CenterTargets>,
) -> Result<FtoCubie, String> {
    cubie_from_facelets_with_optional_center_targets(facelets, center_targets)
}

fn cubie_from_facelets_with_optional_center_targets(
    facelets: &[u8],
    center_targets: Option<&CenterTargets>,
) -> Result<FtoCubie, String> {
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

    let center_care = [true; 12];
    let mut uf = if let Some(targets) = center_targets {
        read_partial_centers(
            facelets,
            &UF_CENTER_FACELETS,
            &center_care,
            0,
            Some(&targets.uf),
            Some(&targets.uf_sources),
            None,
            None,
        )?
    } else {
        read_uf_centers(facelets)?
    };
    let mut rl = if let Some(targets) = center_targets {
        read_partial_centers(
            facelets,
            &RL_CENTER_FACELETS,
            &center_care,
            4,
            Some(&targets.rl),
            Some(&targets.rl_sources),
            Some(&targets.rl_top_sources),
            Some(&RL_LAST_LAYER_FIXED_CENTER_SLOT),
        )?
    } else {
        read_rl_centers(facelets)?
    };
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
    let mut moves = Vec::new();
    for name in names {
        let mv = parse_move(name)?;
        if !moves.contains(&mv) {
            moves.push(mv);
        }
    }
    Ok(moves)
}

fn parse_move(token: &str) -> Result<Move, String> {
    match token {
        "U" => Ok(Move::U),
        "U'" | "Ui" => Ok(Move::Up),
        "F" => Ok(Move::F),
        "F'" | "Fi" => Ok(Move::Fp),
        "BR" | "r" => Ok(Move::BR),
        "BR'" | "BRi" | "r'" | "ri" => Ok(Move::BRp),
        "BL" | "l" => Ok(Move::BL),
        "BL'" | "BLi" | "l'" | "li" => Ok(Move::BLp),
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
        "S" => Ok(Move::S),
        "S'" | "Si" => Ok(Move::Sp),
        "E" => Ok(Move::E),
        "E'" | "Ei" => Ok(Move::Ep),
        "(R U R')" => Ok(Move::RURp),
        "(R U' R')" => Ok(Move::RUpRp),
        "(R' U R)" => Ok(Move::RpUR),
        "(R' U' R)" => Ok(Move::RpUpR),
        "(F U F')" => Ok(Move::FUFp),
        "(F U' F')" => Ok(Move::FUpFp),
        "(F' U F)" => Ok(Move::FpUF),
        "(F' U' F)" => Ok(Move::FpUpF),
        _ => Err(format!("unknown move: {token}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cubie_and_partial_mask_from_facelets, cubie_from_facelets,
        cubie_from_facelets_for_solving, cubie_from_facelets_with_center_targets, CenterTargets,
        CORNER_FACELETS, EDGE_FACELETS, F, U, empty_center_source_groups,
    };
    use fto_core::FtoCubie;

    fn solved_facelets() -> Vec<u8> {
        (0..72).map(|idx| (idx / 9) as u8).collect()
    }

    #[test]
    fn accepts_solved_facelets() {
        let cubie = cubie_from_facelets(&solved_facelets()).expect("solved facelets should parse");
        assert_eq!(cubie, FtoCubie::solved());
    }

    #[test]
    fn accepts_same_orbit_center_swap() {
        let mut facelets = solved_facelets();
        facelets.swap(U + 2, F + 2);
        cubie_from_facelets(&facelets).expect("same-orbit center swap should parse");
    }

    #[test]
    fn accepts_blacked_piece_slot() {
        let mut facelets = solved_facelets();
        for &facelet in &EDGE_FACELETS[0] {
            facelets[facelet] = 8;
        }
        cubie_from_facelets_for_solving(&facelets)
            .expect("partial facelets with a blacked edge slot should parse");
    }

    #[test]
    fn ignored_corner_absorbs_orientation_parity() {
        let mut facelets = solved_facelets();
        for &facelet in &CORNER_FACELETS[0] {
            facelets[facelet] = 8;
        }
        let corner = CORNER_FACELETS[1];
        let original = [
            facelets[corner[0]],
            facelets[corner[1]],
            facelets[corner[2]],
            facelets[corner[3]],
        ];
        for i in 0..4 {
            facelets[corner[i]] = original[(i + 2) % 4];
        }
        cubie_from_facelets_for_solving(&facelets)
            .expect("ignored corner should absorb visible corner orientation parity");
    }

    #[test]
    fn partial_mask_tracks_visible_edge_identity() {
        let mut facelets = solved_facelets();
        for (&a, &b) in EDGE_FACELETS[0].iter().zip(&EDGE_FACELETS[1]) {
            facelets.swap(a, b);
        }
        for &facelet in &EDGE_FACELETS[0] {
            facelets[facelet] = 8;
        }
        for &facelet in &EDGE_FACELETS[2] {
            facelets[facelet] = 8;
        }
        let (_, mask) = cubie_and_partial_mask_from_facelets(&facelets, None, false)
            .expect("partial facelets should parse");
        let mask = mask.expect("black stickers should produce a partial mask");
        assert!(mask.edges[0]);
        assert!(!mask.edges[1]);
        assert!(!mask.edges[2]);
    }

    #[test]
    fn partial_mask_can_pin_single_visible_uf_center_color() {
        let mut facelets = solved_facelets();
        facelets[U + 5] = 8;
        facelets[U + 7] = 8;
        let targets = CenterTargets {
            uf: [Some(1), None, None, None],
            rl: [None; 4],
            uf_sources: [None; 4],
            rl_sources: [None; 4],
            rl_top_sources: empty_center_source_groups(),
        };
        let (_, mask) = cubie_and_partial_mask_from_facelets(&facelets, Some(&targets), false)
            .expect("partial facelets with center target should parse");
        let mask = mask.expect("black stickers should produce a partial mask");
        assert!(!mask.uf_centers[0]);
        assert!(mask.uf_centers[1]);
        assert_eq!(mask.uf_center_targets[0], Some(1));
    }

    #[test]
    fn last_layer_exact_parser_uses_center_target_source() {
        let facelets = solved_facelets();
        let targets = CenterTargets {
            uf: [None; 4],
            rl: [None, None, Some(8), None],
            uf_sources: [None; 4],
            rl_sources: [None, None, Some(7), None],
            rl_top_sources: empty_center_source_groups(),
        };
        let cubie = cubie_from_facelets_with_center_targets(&facelets, Some(&targets))
            .expect("targeted exact facelets should parse");
        assert_eq!(cubie.rl[7], 8);
    }

    #[test]
    fn last_layer_exact_parser_keeps_top_center_source_off_fixed_slot() {
        let facelets = solved_facelets();
        let mut top_sources = empty_center_source_groups();
        top_sources[2].push(7);
        let targets = CenterTargets {
            uf: [None; 4],
            rl: [None; 4],
            uf_sources: [None; 4],
            rl_sources: [None; 4],
            rl_top_sources: top_sources,
        };
        let cubie = cubie_from_facelets_with_center_targets(&facelets, Some(&targets))
            .expect("targeted exact facelets should parse");
        assert_ne!(cubie.rl[7], 8);
        assert_eq!(cubie.rl[7] / 3, 2);
    }

    #[test]
    fn last_layer_mode_without_black_stickers_uses_exact_state() {
        let (_, mask) = cubie_and_partial_mask_from_facelets(&solved_facelets(), None, true)
            .expect("last-layer facelets should parse");
        assert!(mask.is_none());
    }
}
