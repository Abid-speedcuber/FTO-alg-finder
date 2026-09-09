use fto_core::{
    moves::Move,
    FtoCubie,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
        Arc, Mutex,
    },
    thread,
    time::Duration,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolveRequest {
    scramble: Option<String>,
    state: Option<CubieState>,
    facelets: Option<Vec<u8>>,
    allowed_moves: Vec<String>,
    max_depth: Option<u8>,
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
#[serde(rename_all = "camelCase")]
struct PartialMask {
    corners: [bool; 6],
    edges: [bool; 12],
    uf_centers: [bool; 12],
    rl_centers: [bool; 12],
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

#[derive(Default)]
struct SolverState {
    current_cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
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
    tauri::async_runtime::spawn_blocking(move || {
        let response = run_solve_blocking(&request, &cancel, &app, &cancels);
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
fn validate_facelets(facelets: Vec<u8>) -> Result<CubieState, String> {
    cubie_from_facelets_for_solving(&facelets).map(cubie_to_state)
}

fn run_solve_blocking(
    request: &SolveRequest,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
    cancels: &Mutex<Option<Arc<AtomicBool>>>,
) -> () {
    let result = solve_fto_inner(request, cancel, app);

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
) -> Result<SolveResponse, String> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")));

    let allowed_moves = parse_move_names(&request.allowed_moves)?;
    if allowed_moves.is_empty() {
        return Err("at least one move must be allowed".to_owned());
    }

    let cubie = if let Some(state) = request.state.clone() {
        FtoCubie::new(state.cp, state.co, state.ep, state.uf, state.rl)
    } else if let Some(facelets) = request.facelets.clone() {
        cubie_from_facelets_for_solving(&facelets)?
    } else if let Some(scramble) = request.scramble.clone() {
        apply_sequence(FtoCubie::solved(), &scramble)?
    } else {
        FtoCubie::solved()
    };

    run_release_cli(workspace_root, request, &allowed_moves, cubie, cancel, app)
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

fn run_release_cli(
    workspace_root: &Path,
    request: &SolveRequest,
    allowed_moves: &[Move],
    cubie: FtoCubie,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
) -> Result<SolveResponse, String> {
    let exe = workspace_root.join("target/release/fto-cli");
    if !exe.exists() {
        return Err(format!(
            "release solver binary is missing: {}. Run `cargo build --release -p fto-cli` once.",
            exe.display()
        ));
    }

    let mut args = Vec::new();
    if let Some(depth) = request.max_depth {
        args.push("--depth".to_owned());
        args.push(depth.to_string());
        args.push("--exact".to_owned());
    }
    if request.find_all {
        args.push("--all".to_owned());
    }
    if request.restricted_pruning {
        args.push("--restricted-pruning".to_owned());
    }
    if request.threads > 1 {
        args.push("--threads".to_owned());
        args.push(request.threads.to_string());
    }
    if allowed_moves != Move::ALL.as_slice() {
        args.push("--moves".to_owned());
        args.push(allowed_moves.iter().map(|mv| mv.name()).collect::<Vec<_>>().join(" "));
    }

    emit_line(app, "info", &format!("running {}", exe.display()));
    let mut child = Command::new(&exe)
        .args(&args)
        .current_dir(workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start release solver: {error}"))?;

    let partial_mask = request
        .facelets
        .as_ref()
        .and_then(|facelets| partial_mask_from_facelets(facelets).ok())
        .filter(|mask| !mask_is_full(mask));
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(cubie_json_with_mask(cubie, partial_mask.as_ref()).as_bytes())
            .map_err(|error| format!("failed to send state to solver: {error}"))?;
    }

    let stdout = child.stdout.take().ok_or("failed to capture solver stdout")?;
    let stderr = child.stderr.take().ok_or("failed to capture solver stderr")?;
    let (tx, rx) = mpsc::channel::<(String, String)>();

    spawn_line_reader(stdout, "stdout", tx.clone());
    spawn_line_reader(stderr, "stderr", tx);

    let mut stdout_lines = Vec::new();
    loop {
        while let Ok((source, line)) = rx.try_recv() {
            let kind = classify_cli_line(&line, &source);
            emit_line(app, kind, &line);
            if source == "stdout" {
                stdout_lines.push(line);
            }
        }

        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".to_owned());
        }

        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => {
                while let Ok((source, line)) = rx.try_recv() {
                    let kind = classify_cli_line(&line, &source);
                    emit_line(app, kind, &line);
                    if source == "stdout" {
                        stdout_lines.push(line);
                    }
                }
                if !status.success() {
                    return Err(format!("release solver exited with {status}"));
                }
                return parse_cli_response(&stdout_lines);
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn spawn_line_reader<R>(stream: R, source: &'static str, tx: mpsc::Sender<(String, String)>)
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let _ = tx.send((source.to_owned(), line));
        }
    });
}

fn classify_cli_line(line: &str, source: &str) -> &'static str {
    if line.starts_with("searching depth") {
        "search"
    } else if line.starts_with("loading") || line.starts_with("using") || line.starts_with("found solution") {
        "info"
    } else if line.starts_with("nodes:") || line.starts_with("solutions:") {
        "done"
    } else if source == "stderr" {
        "progress"
    } else {
        "solution"
    }
}

fn parse_cli_response(lines: &[String]) -> Result<SolveResponse, String> {
    let mut nodes = 0_u64;
    let mut solution_count = None;
    let mut solutions = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("nodes:") {
            nodes = rest.trim().parse().map_err(|_| "failed to parse solver node count")?;
        } else if let Some(rest) = line.strip_prefix("solutions:") {
            solution_count = Some(
                rest.trim()
                    .parse::<usize>()
                    .map_err(|_| "failed to parse solver solution count")?,
            );
        } else if !line.trim().is_empty() {
            solutions.push(line.clone());
        }
    }
    if solution_count == Some(0) {
        solutions.clear();
    }
    Ok(SolveResponse { nodes, solutions })
}

fn cubie_json_with_mask(cubie: FtoCubie, mask: Option<&PartialMask>) -> String {
    let mut out = format!(
        "{{\"cp\":{},\"co\":{},\"ep\":{},\"uf\":{},\"rl\":{}",
        json_array(&cubie.cp),
        json_array(&cubie.co),
        json_array(&cubie.ep),
        json_array(&cubie.uf),
        json_array(&cubie.rl),
    );
    if let Some(mask) = mask {
        out.push_str(",\"partialMask\":");
        out.push_str(&partial_mask_json(mask));
    }
    out.push('}');
    out
}

fn json_array<const N: usize>(values: &[u8; N]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn partial_mask_json(mask: &PartialMask) -> String {
    format!(
        "{{\"corners\":{},\"edges\":{},\"ufCenters\":{},\"rlCenters\":{}}}",
        bool_array(&mask.corners),
        bool_array(&mask.edges),
        bool_array(&mask.uf_centers),
        bool_array(&mask.rl_centers),
    )
}

fn bool_array<const N: usize>(values: &[bool; N]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn cubie_from_facelets_for_solving(facelets: &[u8]) -> Result<FtoCubie, String> {
    if facelets.iter().any(|&color| color == 8) {
        cubie_from_partial_facelets(facelets).map(|(cubie, _)| cubie)
    } else {
        cubie_from_facelets(facelets)
    }
}

fn cubie_from_partial_facelets(facelets: &[u8]) -> Result<(FtoCubie, PartialMask), String> {
    if facelets.len() != 72 {
        return Err("facelet input must contain exactly 72 stickers".to_owned());
    }
    for &color in facelets {
        if color > 8 {
            return Err(format!("facelet color {color} is outside 0..8"));
        }
    }
    let mask = partial_mask_from_facelets(facelets)?;
    let (mut cp, corner_ori) = detect_partial_pieces(&CORNER_FACELETS, facelets, &mask.corners)
        .map_err(|_| "defined corner stickers do not describe legal FTO corners".to_owned())?;
    let mut co = [0_u8; 6];
    let mut corner_xor = 0_u8;
    for i in 0..6 {
        co[i] = corner_ori[i] >> 1;
        corner_xor ^= co[i];
    }
    if corner_xor != 0 {
        return Err("defined corner orientation parity is invalid".to_owned());
    }

    let (mut ep, _) = detect_partial_pieces(&EDGE_FACELETS, facelets, &mask.edges)
        .map_err(|_| "defined edge stickers do not describe legal FTO edges".to_owned())?;
    repair_partial_parity(&mut cp, &mask.corners)?;
    repair_partial_parity(&mut ep, &mask.edges)?;

    let uf = read_partial_centers(facelets, &UF_CENTER_FACELETS, &mask.uf_centers, 0)?;
    let rl = read_partial_centers(facelets, &RL_CENTER_FACELETS, &mask.rl_centers, 4)?;

    Ok((FtoCubie::new(cp, co, ep, uf, rl), mask))
}

fn partial_mask_from_facelets(facelets: &[u8]) -> Result<PartialMask, String> {
    if facelets.len() != 72 {
        return Err("facelet input must contain exactly 72 stickers".to_owned());
    }
    Ok(PartialMask {
        corners: care_mask(&CORNER_FACELETS, facelets),
        edges: care_mask(&EDGE_FACELETS, facelets),
        uf_centers: center_care_mask(&UF_CENTER_FACELETS, facelets),
        rl_centers: center_care_mask(&RL_CENTER_FACELETS, facelets),
    })
}

fn mask_is_full(mask: &PartialMask) -> bool {
    mask.corners.iter().all(|&care| care)
        && mask.edges.iter().all(|&care| care)
        && mask.uf_centers.iter().all(|&care| care)
        && mask.rl_centers.iter().all(|&care| care)
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
) -> Result<[u8; N], String> {
    let mut used_per_color = [0_u8; 4];
    let mut out = [u8::MAX; N];
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
        if used_per_color[mapped] >= 3 {
            return Err("defined center orbit has too many centers of one color".to_owned());
        }
        out[pos] = mapped as u8 * 3 + used_per_color[mapped];
        used_per_color[mapped] += 1;
    }
    for pos in 0..N {
        if out[pos] != u8::MAX {
            continue;
        }
        let mapped = (0..4)
            .find(|&color| used_per_color[color] < 3)
            .ok_or_else(|| "partial center fill failed".to_owned())?;
        out[pos] = mapped as u8 * 3 + used_per_color[mapped];
        used_per_color[mapped] += 1;
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::{cubie_from_facelets, cubie_from_facelets_for_solving, EDGE_FACELETS, F, U};
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
}
