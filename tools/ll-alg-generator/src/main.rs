use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
    process,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Instant,
    collections::VecDeque,
};

use fto_core::{
    moves::{move_cubies, Move, MOVE_COUNT},
    FtoCubie,
};

const FACE_MOVES: [Move; 16] = [
    Move::U,
    Move::Up,
    Move::F,
    Move::Fp,
    Move::BR,
    Move::BRp,
    Move::BL,
    Move::BLp,
    Move::D,
    Move::Dp,
    Move::B,
    Move::Bp,
    Move::R,
    Move::Rp,
    Move::L,
    Move::Lp,
];

const WIDE_MOVES: [Move; 8] = [
    Move::Uw,
    Move::Uwp,
    Move::Fw,
    Move::Fwp,
    Move::Rw,
    Move::Rwp,
    Move::Lw,
    Move::Lwp,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WideMode {
    Include,
    Exclude,
    Only,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CenterMode {
    ExactPieces,
    SameColor,
}

#[derive(Clone, Debug)]
struct Config {
    max_depth: usize,
    exact_depth: Option<usize>,
    out: PathBuf,
    center_mode: CenterMode,
    allow_trailing_u: bool,
    progress_nodes: u64,
    threads: usize,
    split_depth: usize,
    resume: bool,
    seed_paths: Vec<PathBuf>,
    wide_mode: WideMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_depth: 12,
            exact_depth: None,
            out: PathBuf::from("ll_algs_12_or_under.txt"),
            center_mode: CenterMode::ExactPieces,
            allow_trailing_u: false,
            progress_nodes: 10_000_000,
            threads: thread::available_parallelism().map_or(1, usize::from),
            split_depth: 4,
            resume: false,
            seed_paths: Vec::new(),
            wide_mode: WideMode::Include,
        }
    }
}

#[derive(Clone, Debug)]
struct MoveSet {
    moves: Vec<Move>,
}

#[derive(Clone, Copy, Debug)]
struct LlMask {
    cp: [bool; 6],
    ep: [bool; 12],
    uf: [bool; 12],
    rl: [bool; 12],
}

#[derive(Debug)]
struct Search {
    config: Config,
    move_set: MoveSet,
    move_cubies: [FtoCubie; MOVE_COUNT],
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    pruning: PruningTables,
    ll_mask: LlMask,
    writer: Arc<Mutex<BufWriter<File>>>,
    seen_trimmed_algs: Arc<Mutex<HashSet<Vec<u8>>>>,
    seen_cases: Arc<Mutex<HashSet<Vec<u8>>>>,
    nodes: Arc<AtomicU64>,
    pruned: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    started: Instant,
}

#[derive(Clone, Debug)]
struct Task {
    cubie: FtoCubie,
    path: Vec<Move>,
    last_move: Option<Move>,
    passed_first_non_ud: bool,
    depth_left: usize,
}

struct Worker<'a> {
    config: &'a Config,
    move_set: &'a MoveSet,
    move_cubies: &'a [FtoCubie; MOVE_COUNT],
    commute: &'a [[bool; MOVE_COUNT]; MOVE_COUNT],
    pruning: &'a PruningTables,
    ll_mask: LlMask,
    writer: Arc<Mutex<BufWriter<File>>>,
    seen_trimmed_algs: Arc<Mutex<HashSet<Vec<u8>>>>,
    seen_cases: Arc<Mutex<HashSet<Vec<u8>>>>,
    nodes: Arc<AtomicU64>,
    pruned: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    started: Instant,
    path: Vec<Move>,
}

#[derive(Clone, Debug)]
struct PruningTables {
    corner: [[u8; 12]; 6],
    edge: [[u8; 12]; 12],
    uf_center: [[u8; 12]; 12],
    rl_center: [[u8; 12]; 12],
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn open_output(config: &Config) -> Result<File, String> {
    if config.resume {
        return OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.out)
            .map_err(|error| format!("failed to open {} for append: {error}", config.out.display()));
    }
    File::create(&config.out)
        .map_err(|error| format!("failed to create {}: {error}", config.out.display()))
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let file = open_output(&config)?;
    let move_cubies = move_cubies();
    let move_set = move_set(config.wide_mode);
    let ll_mask = ll_mask_from_u(&move_cubies);
    let pruning = build_pruning_tables(&move_cubies, &move_set);
    let mut search = Search {
        config,
        move_set,
        move_cubies,
        commute: move_commutation(&move_cubies),
        pruning,
        ll_mask,
        writer: Arc::new(Mutex::new(BufWriter::new(file))),
        seen_trimmed_algs: Arc::new(Mutex::new(HashSet::new())),
        seen_cases: Arc::new(Mutex::new(HashSet::new())),
        nodes: Arc::new(AtomicU64::new(0)),
        pruned: Arc::new(AtomicU64::new(0)),
        written: Arc::new(AtomicU64::new(0)),
        started: Instant::now(),
    };
    seed_existing_algs(&search.config, &search.move_cubies, &search.seen_trimmed_algs, &search.seen_cases)?;

    eprintln!(
        "writing {}; max_depth={} center_mode={:?} wide_mode={:?} trailing_u={} threads={}",
        search.config.out.display(),
        search.config.max_depth,
        search.config.center_mode,
        search.config.wide_mode,
        if search.config.allow_trailing_u {
            "allowed"
        } else {
            "canonicalized away"
        },
        search.config.threads
    );

    if let Some(depth) = search.config.exact_depth {
        search.search_depth(depth)?;
    } else {
        for depth in 1..=search.config.max_depth {
            search.search_depth(depth)?;
        }
    }

    search
        .writer
        .lock()
        .expect("writer lock poisoned")
        .flush()
        .map_err(|error| error.to_string())?;
    eprintln!(
        "done: wrote {} algs after visiting {} nodes in {:.1}s",
        search.written.load(Ordering::Relaxed),
        search.nodes.load(Ordering::Relaxed),
        search.started.elapsed().as_secs_f64()
    );
    eprintln!("pruned {} branches", search.pruned.load(Ordering::Relaxed));
    Ok(())
}

fn seed_existing_algs(
    config: &Config,
    moves: &[FtoCubie; MOVE_COUNT],
    seen_trimmed_algs: &Arc<Mutex<HashSet<Vec<u8>>>>,
    seen_cases: &Arc<Mutex<HashSet<Vec<u8>>>>,
) -> Result<(), String> {
    let mut seed_paths = config.seed_paths.clone();
    if config.resume && config.out.exists() {
        seed_paths.push(config.out.clone());
    }
    for path in seed_paths {
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read seed file {}: {error}", path.display()))?;
        let mut seeded = 0_u64;
        for (line_no, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path_moves = parse_alg_line(trimmed)
                .map_err(|error| format!("{}:{}: {error}", path.display(), line_no + 1))?;
            seen_trimmed_algs
                .lock()
                .expect("trimmed-alg lock poisoned")
                .insert(trimmed_auf_path_key(&path_moves));
            let cubie = apply_path(&path_moves, moves);
            seen_cases
                .lock()
                .expect("seen-case lock poisoned")
                .insert(canonical_u_conjugate_key(&cubie, moves));
            seeded += 1;
        }
        eprintln!("seeded {seeded} existing algs from {}", path.display());
    }
    Ok(())
}

impl Search {
    fn search_depth(&mut self, depth: usize) -> Result<(), String> {
        eprintln!("depth {depth}...");
        let mut tasks = Vec::new();
        self.build_tasks(
            FtoCubie::solved(),
            Vec::new(),
            None,
            false,
            depth,
            self.config.split_depth.min(depth),
            &mut tasks,
        );
        eprintln!("depth {depth}: {} split tasks", tasks.len());
        if tasks.is_empty() {
            return Ok(());
        }

        let next_task = AtomicUsize::new(0);
        let tasks = Arc::new(tasks);
        let worker_count = self.config.threads.max(1).min(tasks.len());
        let config = &self.config;
        let move_set = &self.move_set;
        let move_cubies = &self.move_cubies;
        let commute = &self.commute;
        let pruning = &self.pruning;
        let ll_mask = self.ll_mask;
        let started = self.started;
        thread::scope(|scope| {
            for _ in 0..worker_count {
                let tasks = Arc::clone(&tasks);
                let writer = Arc::clone(&self.writer);
                let seen_trimmed_algs = Arc::clone(&self.seen_trimmed_algs);
                let seen_cases = Arc::clone(&self.seen_cases);
                let nodes = Arc::clone(&self.nodes);
                let pruned = Arc::clone(&self.pruned);
                let written = Arc::clone(&self.written);
                let next_task = &next_task;
                scope.spawn(move || {
                    let mut worker = Worker {
                        config,
                        move_set,
                        move_cubies,
                        commute,
                        pruning,
                        ll_mask,
                        writer,
                        seen_trimmed_algs,
                        seen_cases,
                        nodes,
                        pruned,
                        written,
                        started,
                        path: Vec::with_capacity(depth),
                    };
                    loop {
                        let index = next_task.fetch_add(1, Ordering::Relaxed);
                        let Some(task) = tasks.get(index) else {
                            break;
                        };
                        worker.path.clear();
                        worker.path.extend_from_slice(&task.path);
                        worker.dfs(
                            task.cubie,
                            task.depth_left,
                            task.last_move,
                            task.passed_first_non_ud,
                        );
                    }
                });
            }
        });
        Ok(())
    }

    fn build_tasks(
        &mut self,
        cubie: FtoCubie,
        path: Vec<Move>,
        last_move: Option<Move>,
        passed_first_non_ud: bool,
        depth_left: usize,
        split_left: usize,
        tasks: &mut Vec<Task>,
    ) {
        if lower_bound_to_fix(&cubie, self.ll_mask, self.config.center_mode, &self.pruning)
            > depth_left as u8
        {
            self.pruned.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if split_left == 0 || depth_left == 0 {
            tasks.push(Task {
                cubie,
                path,
                last_move,
                passed_first_non_ud,
                depth_left,
            });
            return;
        }

        let move_set = self.move_set.moves.clone();
        for mv in move_set {
            if last_move.is_some_and(|last| self.should_skip_after(last, mv)) {
                continue;
            }
            let next_passed_first = match first_non_ud_status(passed_first_non_ud, mv, self.config.wide_mode) {
                FirstNonUd::Allowed(value) => value,
                FirstNonUd::Rejected => continue,
            };
            if !self.config.allow_trailing_u && depth_left == 1 && is_u(mv) {
                continue;
            }

            let mut next_path = path.clone();
            next_path.push(mv);
            let next = cubie.compose(&self.move_cubies[mv.idx()]);
            self.build_tasks(
                next,
                next_path,
                Some(mv),
                next_passed_first,
                depth_left - 1,
                split_left - 1,
                tasks,
            );
        }
    }

    fn should_skip_after(&self, last: Move, current: Move) -> bool {
        last.axis() == current.axis()
            || (self.commute[last.idx()][current.idx()] && current.idx() < last.idx())
    }
}

impl Worker<'_> {
    fn dfs(
        &mut self,
        cubie: FtoCubie,
        depth_left: usize,
        last_move: Option<Move>,
        passed_first_non_ud: bool,
    ) {
        let nodes = self.nodes.fetch_add(1, Ordering::Relaxed) + 1;
        if self.config.progress_nodes != 0 && nodes % self.config.progress_nodes == 0 {
            eprintln!(
                "progress: {} nodes, {} pruned, {} algs, current depth {}, elapsed {:.1}s",
                nodes,
                self.pruned.load(Ordering::Relaxed),
                self.written.load(Ordering::Relaxed),
                self.path.len(),
                self.started.elapsed().as_secs_f64()
            );
        }

        if lower_bound_to_fix(&cubie, self.ll_mask, self.config.center_mode, self.pruning)
            > depth_left as u8
        {
            self.pruned.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if depth_left == 0 {
            if passed_first_non_ud && self.only_affects_ll(&cubie) {
                self.write_path(&cubie);
            }
            return;
        }

        for &mv in &self.move_set.moves {
            if last_move.is_some_and(|last| self.should_skip_after(last, mv)) {
                continue;
            }
            let next_passed_first = match first_non_ud_status(passed_first_non_ud, mv, self.config.wide_mode) {
                FirstNonUd::Allowed(value) => value,
                FirstNonUd::Rejected => continue,
            };
            if !self.config.allow_trailing_u && depth_left == 1 && is_u(mv) {
                continue;
            }

            self.path.push(mv);
            let next = cubie.compose(&self.move_cubies[mv.idx()]);
            self.dfs(next, depth_left - 1, Some(mv), next_passed_first);
            self.path.pop();
        }
    }

    fn should_skip_after(&self, last: Move, current: Move) -> bool {
        last.axis() == current.axis()
            || (self.commute[last.idx()][current.idx()] && current.idx() < last.idx())
    }

    fn only_affects_ll(&self, cubie: &FtoCubie) -> bool {
        for pos in 0..6 {
            if !self.ll_mask.cp[pos] && (cubie.cp[pos] != pos as u8 || cubie.co[pos] != 0) {
                return false;
            }
        }
        for pos in 0..12 {
            if !self.ll_mask.ep[pos] && cubie.ep[pos] != pos as u8 {
                return false;
            }
            if !self.center_ok(cubie.uf[pos], pos, self.ll_mask.uf[pos]) {
                return false;
            }
            if !self.center_ok(cubie.rl[pos], pos, self.ll_mask.rl[pos]) {
                return false;
            }
        }
        true
    }

    fn center_ok(&self, piece: u8, pos: usize, is_ll_pos: bool) -> bool {
        if is_ll_pos {
            return true;
        }
        match self.config.center_mode {
            CenterMode::ExactPieces => piece == pos as u8,
            CenterMode::SameColor => piece / 3 == pos as u8 / 3,
        }
    }

    fn write_path(&mut self, cubie: &FtoCubie) {
        let trimmed_alg_key = trimmed_auf_path_key(&self.path);
        if !self
            .seen_trimmed_algs
            .lock()
            .expect("trimmed-alg lock poisoned")
            .insert(trimmed_alg_key)
        {
            return;
        }

        let key = canonical_u_conjugate_key(cubie, self.move_cubies);
        if !self
            .seen_cases
            .lock()
            .expect("seen-case lock poisoned")
            .insert(key)
        {
            return;
        }

        let mut line = String::new();
        for (i, mv) in self.path.iter().enumerate() {
            if i != 0 {
                line.push(' ');
            }
            line.push_str(mv.name());
        }
        line.push('\n');
        let mut writer = self.writer.lock().expect("writer lock poisoned");
        writer.write_all(line.as_bytes()).expect("failed to write alg");
        writer.flush().expect("failed to flush alg");
        self.written.fetch_add(1, Ordering::Relaxed);
    }
}

fn trimmed_auf_path_key(path: &[Move]) -> Vec<u8> {
    let mut start = 0;
    let mut end = path.len();
    while start < end && is_u(path[start]) {
        start += 1;
    }
    while end > start && is_u(path[end - 1]) {
        end -= 1;
    }
    path[start..end].iter().map(|mv| mv.idx() as u8).collect()
}

fn parse_alg_line(line: &str) -> Result<Vec<Move>, String> {
    line.split_whitespace().map(move_from_name).collect()
}

fn move_from_name(name: &str) -> Result<Move, String> {
    match name {
        "U" => Ok(Move::U),
        "U'" => Ok(Move::Up),
        "F" => Ok(Move::F),
        "F'" => Ok(Move::Fp),
        "BR" => Ok(Move::BR),
        "BR'" => Ok(Move::BRp),
        "BL" => Ok(Move::BL),
        "BL'" => Ok(Move::BLp),
        "D" => Ok(Move::D),
        "D'" => Ok(Move::Dp),
        "B" => Ok(Move::B),
        "B'" => Ok(Move::Bp),
        "R" => Ok(Move::R),
        "R'" => Ok(Move::Rp),
        "L" => Ok(Move::L),
        "L'" => Ok(Move::Lp),
        "Uw" => Ok(Move::Uw),
        "Uw'" => Ok(Move::Uwp),
        "Fw" => Ok(Move::Fw),
        "Fw'" => Ok(Move::Fwp),
        "Rw" => Ok(Move::Rw),
        "Rw'" => Ok(Move::Rwp),
        "Lw" => Ok(Move::Lw),
        "Lw'" => Ok(Move::Lwp),
        _ => Err(format!("unknown move '{name}'")),
    }
}

fn apply_path(path: &[Move], moves: &[FtoCubie; MOVE_COUNT]) -> FtoCubie {
    let mut cubie = FtoCubie::solved();
    for &mv in path {
        cubie = cubie.compose(&moves[mv.idx()]);
    }
    cubie
}

fn canonical_u_conjugate_key(cubie: &FtoCubie, moves: &[FtoCubie; MOVE_COUNT]) -> Vec<u8> {
    let solved = FtoCubie::solved();
    let u = moves[Move::U.idx()];
    let up = moves[Move::Up.idx()];
    let candidates = [
        solved.compose(cubie).compose(&solved),
        u.compose(cubie).compose(&up),
        up.compose(cubie).compose(&u),
    ];
    candidates
        .iter()
        .map(cubie_key)
        .min()
        .expect("at least one U conjugate")
}

fn cubie_key(cubie: &FtoCubie) -> Vec<u8> {
    let mut key = Vec::with_capacity(54);
    key.extend_from_slice(&cubie.cp);
    key.extend_from_slice(&cubie.co);
    key.extend_from_slice(&cubie.ep);
    key.extend_from_slice(&cubie.uf);
    key.extend_from_slice(&cubie.rl);
    key
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FirstNonUd {
    Allowed(bool),
    Rejected,
}

fn first_non_ud_status(passed_first_non_ud: bool, mv: Move, wide_mode: WideMode) -> FirstNonUd {
    if passed_first_non_ud {
        return FirstNonUd::Allowed(true);
    }
    if is_u(mv) {
        return FirstNonUd::Allowed(false);
    }
    if is_allowed_first_counted_move(mv, wide_mode) {
        return FirstNonUd::Allowed(true);
    }
    FirstNonUd::Rejected
}

fn is_allowed_first_counted_move(mv: Move, wide_mode: WideMode) -> bool {
    if matches!(mv, Move::R | Move::Rp | Move::F | Move::Fp) {
        return wide_mode != WideMode::Only;
    }
    matches!(
        mv,
        Move::Uw | Move::Uwp | Move::Fw | Move::Fwp | Move::Rw | Move::Rwp | Move::Lw | Move::Lwp
    ) && wide_mode != WideMode::Exclude
}

fn is_u(mv: Move) -> bool {
    matches!(mv, Move::U | Move::Up)
}

fn move_set(wide_mode: WideMode) -> MoveSet {
    let mut moves = Vec::new();
    if wide_mode != WideMode::Only {
        moves.extend_from_slice(&FACE_MOVES);
    } else {
        moves.extend_from_slice(&[Move::U, Move::Up]);
    }
    if wide_mode != WideMode::Exclude {
        moves.extend_from_slice(&WIDE_MOVES);
    }
    MoveSet { moves }
}

fn ll_mask_from_u(moves: &[FtoCubie; MOVE_COUNT]) -> LlMask {
    let solved = FtoCubie::solved();
    let u = solved.compose(&moves[Move::U.idx()]);
    let mut mask = LlMask {
        cp: [false; 6],
        ep: [false; 12],
        uf: [false; 12],
        rl: [false; 12],
    };

    for pos in 0..6 {
        mask.cp[pos] = u.cp[pos] != solved.cp[pos] || u.co[pos] != solved.co[pos];
    }
    for pos in 0..12 {
        mask.ep[pos] = u.ep[pos] != solved.ep[pos];
        mask.uf[pos] = u.uf[pos] != solved.uf[pos];
        mask.rl[pos] = u.rl[pos] != solved.rl[pos];
    }
    mask
}

fn move_commutation(moves: &[FtoCubie; MOVE_COUNT]) -> [[bool; MOVE_COUNT]; MOVE_COUNT] {
    let mut commute = [[false; MOVE_COUNT]; MOVE_COUNT];
    for i in 0..MOVE_COUNT {
        for j in 0..MOVE_COUNT {
            commute[i][j] = moves[i].compose(&moves[j]) == moves[j].compose(&moves[i]);
        }
    }
    commute
}

fn build_pruning_tables(moves: &[FtoCubie; MOVE_COUNT], move_set: &MoveSet) -> PruningTables {
    PruningTables {
        corner: build_corner_piece_distances(moves, move_set),
        edge: build_orbit_piece_distances(moves, move_set, Orbit::Edge),
        uf_center: build_orbit_piece_distances(moves, move_set, Orbit::UfCenter),
        rl_center: build_orbit_piece_distances(moves, move_set, Orbit::RlCenter),
    }
}

fn lower_bound_to_fix(
    cubie: &FtoCubie,
    ll_mask: LlMask,
    center_mode: CenterMode,
    pruning: &PruningTables,
) -> u8 {
    let mut bound = 0_u8;

    for home in 0..6 {
        if ll_mask.cp[home] {
            continue;
        }
        let Some((pos, ori)) = corner_pos(cubie, home as u8) else {
            return u8::MAX;
        };
        bound = bound.max(pruning.corner[home][pos * 2 + usize::from(ori)]);
    }

    for home in 0..12 {
        if !ll_mask.ep[home] {
            let Some(pos) = piece_pos(&cubie.ep, home as u8) else {
                return u8::MAX;
            };
            bound = bound.max(pruning.edge[home][pos]);
        }
    }

    if center_mode == CenterMode::ExactPieces {
        for home in 0..12 {
            if !ll_mask.uf[home] {
                let Some(pos) = piece_pos(&cubie.uf, home as u8) else {
                    return u8::MAX;
                };
                bound = bound.max(pruning.uf_center[home][pos]);
            }
            if !ll_mask.rl[home] {
                let Some(pos) = piece_pos(&cubie.rl, home as u8) else {
                    return u8::MAX;
                };
                bound = bound.max(pruning.rl_center[home][pos]);
            }
        }
    }

    bound
}

fn corner_pos(cubie: &FtoCubie, piece: u8) -> Option<(usize, u8)> {
    for pos in 0..6 {
        if cubie.cp[pos] == piece {
            return Some((pos, cubie.co[pos]));
        }
    }
    None
}

fn piece_pos(perm: &[u8; 12], piece: u8) -> Option<usize> {
    perm.iter().position(|&value| value == piece)
}

fn build_corner_piece_distances(
    moves: &[FtoCubie; MOVE_COUNT],
    move_set: &MoveSet,
) -> [[u8; 12]; 6] {
    let mut distances = [[u8::MAX; 12]; 6];
    for home in 0..6 {
        let mut queue = VecDeque::new();
        let start = home * 2;
        distances[home][start] = 0;
        queue.push_back(start);

        while let Some(state) = queue.pop_front() {
            let pos = state / 2;
            let ori = (state % 2) as u8;
            let distance = distances[home][state];
            for &mv in &move_set.moves {
                let (next_pos, next_ori) = move_corner_piece(pos, ori, &moves[mv.idx()]);
                let next = next_pos * 2 + usize::from(next_ori);
                if distances[home][next] == u8::MAX {
                    distances[home][next] = distance + 1;
                    queue.push_back(next);
                }
            }
        }
    }
    distances
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Orbit {
    Edge,
    UfCenter,
    RlCenter,
}

fn build_orbit_piece_distances(
    moves: &[FtoCubie; MOVE_COUNT],
    move_set: &MoveSet,
    orbit: Orbit,
) -> [[u8; 12]; 12] {
    let mut distances = [[u8::MAX; 12]; 12];
    for home in 0..12 {
        let mut queue = VecDeque::new();
        distances[home][home] = 0;
        queue.push_back(home);

        while let Some(pos) = queue.pop_front() {
            let distance = distances[home][pos];
            for &mv in &move_set.moves {
                let next = move_orbit_piece(pos, orbit_perm(&moves[mv.idx()], orbit));
                if distances[home][next] == u8::MAX {
                    distances[home][next] = distance + 1;
                    queue.push_back(next);
                }
            }
        }
    }
    distances
}

fn move_corner_piece(pos: usize, ori: u8, mv: &FtoCubie) -> (usize, u8) {
    for dst in 0..6 {
        if mv.cp[dst] as usize == pos {
            return (dst, ori ^ mv.co[dst]);
        }
    }
    unreachable!("corner move permutation missing source position")
}

fn move_orbit_piece(pos: usize, perm: &[u8; 12]) -> usize {
    for dst in 0..12 {
        if perm[dst] as usize == pos {
            return dst;
        }
    }
    unreachable!("orbit move permutation missing source position")
}

fn orbit_perm(cubie: &FtoCubie, orbit: Orbit) -> &[u8; 12] {
    match orbit {
        Orbit::Edge => &cubie.ep,
        Orbit::UfCenter => &cubie.uf,
        Orbit::RlCenter => &cubie.rl,
    }
}

fn parse_args() -> Result<Config, String> {
    let mut config = Config::default();
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--max-depth" => {
                i += 1;
                config.max_depth = parse_usize(args.get(i), "--max-depth")?;
            }
            "--exact-depth" => {
                i += 1;
                config.exact_depth = Some(parse_usize(args.get(i), "--exact-depth")?);
            }
            "--out" => {
                i += 1;
                config.out = PathBuf::from(args.get(i).ok_or("--out needs a path")?);
            }
            "--center-mode" => {
                i += 1;
                config.center_mode = match args.get(i).ok_or("--center-mode needs a value")?.as_str() {
                    "exact" => CenterMode::ExactPieces,
                    "color" => CenterMode::SameColor,
                    other => {
                        return Err(format!(
                            "--center-mode must be 'exact' or 'color', got '{other}'"
                        ));
                    }
                };
            }
            "--allow-trailing-u" => config.allow_trailing_u = true,
            "--progress-nodes" => {
                i += 1;
                config.progress_nodes = parse_u64(args.get(i), "--progress-nodes")?;
            }
            "--threads" => {
                i += 1;
                config.threads = parse_usize(args.get(i), "--threads")?;
            }
            "--split-depth" => {
                i += 1;
                config.split_depth = parse_usize(args.get(i), "--split-depth")?;
            }
            "--resume" => config.resume = true,
            "--seed" => {
                i += 1;
                config
                    .seed_paths
                    .push(PathBuf::from(args.get(i).ok_or("--seed needs a path")?));
            }
            "--wide-moves" => {
                i += 1;
                config.wide_mode = parse_wide_mode(args.get(i).ok_or("--wide-moves needs a value")?)?;
            }
            "--no-wide-moves" => config.wide_mode = WideMode::Exclude,
            "--only-wide-moves" => config.wide_mode = WideMode::Only,
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    if let Some(depth) = config.exact_depth {
        config.max_depth = depth;
    }
    if config.max_depth == 0 {
        return Err("--max-depth must be at least 1".to_owned());
    }
    if config.threads == 0 {
        return Err("--threads must be at least 1".to_owned());
    }
    Ok(config)
}

fn parse_usize(value: Option<&String>, name: &str) -> Result<usize, String> {
    value
        .ok_or_else(|| format!("{name} needs a value"))?
        .parse()
        .map_err(|_| format!("{name} must be a non-negative integer"))
}

fn parse_u64(value: Option<&String>, name: &str) -> Result<u64, String> {
    value
        .ok_or_else(|| format!("{name} needs a value"))?
        .parse()
        .map_err(|_| format!("{name} must be a non-negative integer"))
}

fn parse_wide_mode(value: &str) -> Result<WideMode, String> {
    match value {
        "include" => Ok(WideMode::Include),
        "exclude" => Ok(WideMode::Exclude),
        "only" => Ok(WideMode::Only),
        other => Err(format!(
            "--wide-moves must be include, exclude, or only, got '{other}'"
        )),
    }
}

fn print_help() {
    println!(
        "Generate primitive FTO algorithms that only affect U-layer pieces.\n\
\n\
Usage:\n\
  cargo run --release -- [options]\n\
\n\
Options:\n\
  --max-depth N          Generate all lengths 1..N (default: 12)\n\
  --exact-depth N        Generate only length N\n\
  --out PATH             Output .txt path (default: ll_algs_12_or_under.txt)\n\
  --center-mode MODE     exact or color (default: exact)\n\
  --allow-trailing-u     Keep algs ending in U/U' instead of canonicalizing them away\n\
  --progress-nodes N     Print progress every N nodes; 0 disables (default: 10000000)\n\
  --threads N            Worker threads (default: available CPU parallelism)\n\
  --split-depth N        Prefix depth used to split worker tasks (default: 4)\n\
  --resume               Append to --out and seed dedupe from existing output\n\
  --seed PATH            Seed dedupe from an existing output file; repeatable\n\
  --wide-moves MODE      include, exclude, or only (default: include)\n\
  --no-wide-moves        Alias for --wide-moves exclude\n\
  --only-wide-moves      Alias for --wide-moves only\n\
  --help                 Show this help\n\
\n\
Canonical rules:\n\
  - primitive moves only: U U' F F' BR BR' BL BL' D D' B B' R R' L L' Uw Uw' Fw Fw' Rw Rw' Lw Lw'\n\
  - before the first counted move, only U U' are allowed\n\
  - the first counted move must be R, R', F, F', Uw, Uw', Fw, Fw', Rw, Rw', Lw, or Lw'\n\
  - by default, final U/U' is omitted because last-layer AUF is free\n\
  - algs with the same text after trimming leading/trailing U/U' are deduped\n\
  - U-conjugate cases are also deduped, so U alg U' variants are not printed"
    );
}
