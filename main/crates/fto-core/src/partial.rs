use crate::{
    moves::{move_cubies, Move, MOVE_COUNT},
    search::{format_solution, SearchConfig, SearchResult},
    FtoCubie,
};
use std::{collections::VecDeque, thread};

const UNVISITED: u8 = u8::MAX;
const INVALID_TRANSITION: u32 = u32::MAX;
const EDGE_TABLE_PIECES: usize = 5;
const CORNER_TABLE_PIECES: usize = 5;
const MAX_DYNAMIC_TABLES: usize = 6;
const MAX_DYNAMIC_TRANSITION_BYTES: usize = 96 * 1024 * 1024;
const RL_LAST_LAYER_FIXED_CENTER_SLOT: [Option<u8>; 4] = [None, Some(3), Some(8), Some(10)];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialMask {
    pub corners: [bool; 6],
    pub edges: [bool; 12],
    pub uf_centers: [bool; 12],
    pub rl_centers: [bool; 12],
    pub uf_center_targets: [Option<u8>; 4],
    pub rl_center_targets: [Option<u8>; 4],
    pub last_layer_centers: bool,
}

impl PartialMask {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            corners: [true; 6],
            edges: [true; 12],
            uf_centers: [true; 12],
            rl_centers: [true; 12],
            uf_center_targets: [None; 4],
            rl_center_targets: [None; 4],
            last_layer_centers: false,
        }
    }

    #[must_use]
    pub fn is_full(&self) -> bool {
        self.corners.iter().all(|&v| v)
            && self.edges.iter().all(|&v| v)
            && self.uf_centers.iter().all(|&v| v)
            && self.rl_centers.iter().all(|&v| v)
            && self.uf_center_targets.iter().all(Option::is_none)
            && self.rl_center_targets.iter().all(Option::is_none)
            && !self.last_layer_centers
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialProblem {
    pub cubie: FtoCubie,
    pub mask: PartialMask,
}

#[must_use]
pub fn solve_partial(problem: &PartialProblem, config: &SearchConfig) -> SearchResult {
    solve_partial_threads(problem, config, 1)
}

#[must_use]
pub fn solve_partial_threads(
    problem: &PartialProblem,
    config: &SearchConfig,
    threads: usize,
) -> SearchResult {
    let mut pruning = DynamicPruning::build(&problem.mask, &config.allowed_moves);
    pruning.build_transitions(MAX_DYNAMIC_TRANSITION_BYTES);
    eprintln!(
        "using {} dynamic partial pruning tables ({:.1} MiB)",
        pruning.tables.len(),
        pruning.bytes() as f64 / (1024.0 * 1024.0)
    );
    for table in &pruning.tables {
        eprintln!("  {}", table.name());
    }

    let mut total = SearchResult {
        solutions: Vec::new(),
        nodes: 0,
    };
    let start_depth = config.min_depth.max(if config.free_u_ends {
        partial_start_states(problem.cubie, &move_cubies(), true)
            .into_iter()
            .map(|(_, state, _)| adjusted_pruning_value(pruning.heuristic(&state), true))
            .min()
            .unwrap_or(0)
    } else {
        pruning.heuristic(&problem.cubie)
    });
    for depth in start_depth..=config.max_depth {
        eprintln!("searching depth {depth}...");
        let depth_result = search_indexed_threads(problem, &pruning, config, depth, threads);
        let depth_nodes = depth_result.nodes;
        total.nodes += depth_nodes;
        total.solutions.extend(depth_result.solutions);
        dedup_solutions(&mut total.solutions);
        if !total.solutions.is_empty() {
            eprintln!("found solution at depth {depth}");
            break;
        }
    }
    total
}

fn search_indexed_threads(
    problem: &PartialProblem,
    pruning: &DynamicPruning,
    config: &SearchConfig,
    depth: u8,
    threads: usize,
) -> SearchResult {
    if threads <= 1 || depth <= 1 {
        return search_indexed(problem, pruning, config, depth);
    }

    let moves = move_cubies();
    let commute = move_commutation();
    let child_depth = depth - 1;
    let mut roots = Vec::new();
    let mut root_nodes = 0_u64;

    for (prefix, start, last_move) in
        partial_start_states(problem.cubie, &moves, config.free_u_ends)
    {
        root_nodes += 1;
        let start_indices = pruning.indices_of_state(&start);
        if adjusted_pruning_value(
            pruning.heuristic_for_indices(&start, &start_indices),
            config.free_u_ends,
        ) > depth
        {
            continue;
        }

        for &mv in &config.allowed_moves {
            if config.free_u_ends && is_u_turn(mv) {
                continue;
            }
            if last_move.is_some_and(|last| should_skip_after(&commute, last, mv)) {
                continue;
            }
            let next_cubie = start.compose(&moves[mv.idx()]);
            let next_indices = pruning.move_indices(&next_cubie, &start_indices, mv);
            if adjusted_pruning_value(
                pruning.heuristic_for_indices(&next_cubie, &next_indices),
                config.free_u_ends,
            ) > child_depth
            {
                continue;
            }
            roots.push((
                prefix,
                mv,
                IndexedPartialState {
                    cubie: next_cubie,
                    indices: next_indices,
                },
            ));
        }
    }

    if roots.is_empty() {
        return SearchResult {
            solutions: Vec::new(),
            nodes: root_nodes,
        };
    }

    let worker_count = threads.min(roots.len());
    let chunk_size = roots.len().div_ceil(worker_count);
    let mut total = SearchResult {
        solutions: Vec::new(),
        nodes: root_nodes,
    };

    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in roots.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                let mut ctx = IndexedPartialSearchContext {
                    problem,
                    pruning,
                    config,
                    moves,
                    commute,
                    path: Vec::with_capacity(depth as usize),
                    free_prefix: None,
                    solutions: Vec::new(),
                    nodes: 0,
                };
                for (prefix, mv, state) in chunk {
                    ctx.free_prefix = *prefix;
                    ctx.path.push(*mv);
                    ctx.dfs(state.clone(), child_depth, Some(*mv));
                    ctx.path.pop();
                    ctx.free_prefix = None;
                    if !config.find_all && !ctx.solutions.is_empty() {
                        break;
                    }
                }
                SearchResult {
                    solutions: ctx.solutions,
                    nodes: ctx.nodes,
                }
            }));
        }

        for handle in handles {
            let result = handle.join().expect("partial search worker panicked");
            total.nodes += result.nodes;
            total.solutions.extend(result.solutions);
        }
    });

    dedup_solutions(&mut total.solutions);
    total
}

fn search_indexed(
    problem: &PartialProblem,
    pruning: &DynamicPruning,
    config: &SearchConfig,
    depth: u8,
) -> SearchResult {
    let mut ctx = IndexedPartialSearchContext {
        problem,
        pruning,
        config,
        moves: move_cubies(),
        commute: move_commutation(),
        path: Vec::with_capacity(depth as usize),
        free_prefix: None,
        solutions: Vec::new(),
        nodes: 0,
    };
    for (prefix, start, last_move) in
        partial_start_states(problem.cubie, &ctx.moves, config.free_u_ends)
    {
        let root = IndexedPartialState {
            cubie: start,
            indices: pruning.indices_of_state(&start),
        };
        ctx.free_prefix = prefix;
        ctx.dfs(root, depth, last_move);
        ctx.free_prefix = None;
        if !config.find_all && !ctx.solutions.is_empty() {
            break;
        }
    }
    SearchResult {
        solutions: ctx.solutions,
        nodes: ctx.nodes,
    }
}

#[derive(Clone)]
struct IndexedPartialState {
    cubie: FtoCubie,
    indices: Vec<usize>,
}

struct IndexedPartialSearchContext<'a> {
    problem: &'a PartialProblem,
    pruning: &'a DynamicPruning,
    config: &'a SearchConfig,
    moves: [FtoCubie; MOVE_COUNT],
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    path: Vec<Move>,
    free_prefix: Option<Move>,
    solutions: Vec<Vec<Move>>,
    nodes: u64,
}

impl IndexedPartialSearchContext<'_> {
    fn dfs(&mut self, state: IndexedPartialState, depth_left: u8, last_move: Option<Move>) {
        if self
            .config
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
        {
            return;
        }
        self.nodes += 1;
        if adjusted_pruning_value(
            self.pruning
                .heuristic_for_indices(&state.cubie, &state.indices),
            self.config.free_u_ends,
        ) > depth_left
        {
            return;
        }
        if depth_left == 0 {
            if self.config.free_u_ends && ends_in_u_or_up(&self.path) {
                return;
            }
            if let Some(suffix) = partial_free_u_suffix(
                state.cubie,
                &self.moves,
                &self.problem.mask,
                self.config.free_u_ends,
            )
            {
                push_unique_solution(
                    &mut self.solutions,
                    free_auf_solution(self.free_prefix, &self.path, suffix),
                );
            }
            return;
        }

        for &mv in &self.config.allowed_moves {
            if self.config.free_u_ends && self.path.is_empty() && is_u_turn(mv) {
                continue;
            }
            if last_move.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            let next_cubie = state.cubie.compose(&self.moves[mv.idx()]);
            let next_indices = self.pruning.move_indices(&next_cubie, &state.indices, mv);
            if adjusted_pruning_value(
                self.pruning
                    .heuristic_for_indices(&next_cubie, &next_indices),
                self.config.free_u_ends,
            ) > depth_left - 1
            {
                continue;
            }
            self.path.push(mv);
            self.dfs(
                IndexedPartialState {
                    cubie: next_cubie,
                    indices: next_indices,
                },
                depth_left - 1,
                Some(mv),
            );
            self.path.pop();
            if !self.config.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }
}

fn is_partial_solved(state: &FtoCubie, mask: &PartialMask) -> bool {
    for piece in 0..6 {
        if mask.corners[piece] && (state.cp[piece] != piece as u8 || state.co[piece] != 0) {
            return false;
        }
    }
    for piece in 0..12 {
        if mask.edges[piece] && state.ep[piece] != piece as u8 {
            return false;
        }
        if mask.uf_centers[piece]
            && !center_piece_satisfies_constraint(
                &state.uf,
                piece as u8,
                &mask.uf_center_targets,
                false,
            )
        {
            return false;
        }
        if mask.rl_centers[piece]
            && !center_piece_satisfies_constraint(
                &state.rl,
                piece as u8,
                &mask.rl_center_targets,
                mask.last_layer_centers,
            )
        {
            return false;
        }
    }
    true
}

fn center_piece_satisfies_constraint(
    orbit: &[u8; 12],
    piece: u8,
    targets: &[Option<u8>; 4],
    last_layer_rl: bool,
) -> bool {
    let color = piece / 3;
    if let Some(target_slot) = targets[color as usize] {
        return orbit[target_slot as usize] == piece;
    }
    if last_layer_rl {
        if let Some(fixed_slot) = RL_LAST_LAYER_FIXED_CENTER_SLOT[color as usize] {
            return orbit
                .iter()
                .position(|&candidate| candidate == piece)
                .is_some_and(|pos| {
                    if piece == fixed_slot {
                        pos == fixed_slot as usize
                    } else {
                        pos as u8 / 3 == color && pos != fixed_slot as usize
                    }
                });
        }
    }
    center_piece_is_solved_by_color(orbit, piece)
}

fn center_piece_is_solved_by_color(orbit: &[u8; 12], piece: u8) -> bool {
    orbit
        .iter()
        .position(|&candidate| candidate == piece)
        .is_some_and(|pos| pos as u8 / 3 == piece / 3)
}

fn adjusted_pruning_value(value: u8, free_u_ends: bool) -> u8 {
    if free_u_ends {
        value.saturating_sub(1)
    } else {
        value
    }
}

fn partial_start_states(
    root: FtoCubie,
    moves: &[FtoCubie; MOVE_COUNT],
    free_u_ends: bool,
) -> Vec<(Option<Move>, FtoCubie, Option<Move>)> {
    if !free_u_ends {
        return vec![(None, root, None)];
    }
    vec![
        (None, root, None),
        (Some(Move::U), root.compose(&moves[Move::U.idx()]), Some(Move::U)),
        (
            Some(Move::Up),
            root.compose(&moves[Move::Up.idx()]),
            Some(Move::Up),
        ),
    ]
}

fn partial_free_u_suffix(
    state: FtoCubie,
    moves: &[FtoCubie; MOVE_COUNT],
    mask: &PartialMask,
    free_u_ends: bool,
) -> Option<Option<Move>> {
    if is_partial_solved(&state, mask) {
        return Some(None);
    }
    if !free_u_ends {
        return None;
    }
    if is_partial_solved(&state.compose(&moves[Move::U.idx()]), mask) {
        return Some(Some(Move::U));
    }
    if is_partial_solved(&state.compose(&moves[Move::Up.idx()]), mask) {
        return Some(Some(Move::Up));
    }
    None
}

fn ends_in_u_or_up(path: &[Move]) -> bool {
    matches!(path.last(), Some(&Move::U) | Some(&Move::Up))
}

fn is_u_turn(mv: Move) -> bool {
    matches!(mv, Move::U | Move::Up)
}

fn push_unique_solution(solutions: &mut Vec<Vec<Move>>, solution: Vec<Move>) {
    if !solutions.contains(&solution) {
        solutions.push(solution);
    }
}

fn free_auf_solution(prefix: Option<Move>, path: &[Move], suffix: Option<Move>) -> Vec<Move> {
    let mut solution = Vec::with_capacity(
        path.len() + usize::from(prefix.is_some()) + usize::from(suffix.is_some()),
    );
    if let Some(prefix) = prefix {
        solution.push(prefix);
    }
    solution.extend_from_slice(path);
    if let Some(suffix) = suffix {
        solution.push(suffix);
    }
    solution
}

fn dedup_solutions(solutions: &mut Vec<Vec<Move>>) {
    let mut unique = Vec::with_capacity(solutions.len());
    for solution in solutions.drain(..) {
        push_unique_solution(&mut unique, solution);
    }
    *solutions = unique;
}

struct DynamicPruning {
    tables: Vec<DynamicTable>,
}

impl DynamicPruning {
    fn build(mask: &PartialMask, allowed_moves: &[Move]) -> Self {
        let mut tables = Vec::new();
        let cared_edges = mask
            .edges
            .iter()
            .enumerate()
            .filter_map(|(piece, &care)| care.then_some(piece as u8))
            .collect::<Vec<_>>();
        let cared_corners = mask
            .corners
            .iter()
            .enumerate()
            .filter_map(|(piece, &care)| care.then_some(piece as u8))
            .collect::<Vec<_>>();

        add_piece_tables(
            &mut tables,
            DynamicKind::Edge,
            &cared_edges,
            EDGE_TABLE_PIECES,
            allowed_moves,
        );
        add_piece_tables(
            &mut tables,
            DynamicKind::Corner,
            &cared_corners,
            CORNER_TABLE_PIECES,
            allowed_moves,
        );
        Self { tables }
    }

    fn heuristic(&self, state: &FtoCubie) -> u8 {
        self.tables
            .iter()
            .map(|table| table.value(state))
            .max()
            .unwrap_or(0)
    }

    fn heuristic_for_indices(&self, state: &FtoCubie, indices: &[usize]) -> u8 {
        self.tables
            .iter()
            .enumerate()
            .map(|(idx, table)| table.value_for_index_or_state(indices[idx], state))
            .max()
            .unwrap_or(0)
    }

    fn indices_of_state(&self, state: &FtoCubie) -> Vec<usize> {
        self.tables
            .iter()
            .map(|table| table.index_of_state(state))
            .collect()
    }

    fn move_indices(&self, state: &FtoCubie, indices: &[usize], mv: Move) -> Vec<usize> {
        self.tables
            .iter()
            .enumerate()
            .map(|(idx, table)| table.move_cached_index(indices[idx], state, mv))
            .collect()
    }

    fn bytes(&self) -> usize {
        self.tables.iter().map(DynamicTable::bytes).sum()
    }

    fn build_transitions(&mut self, max_bytes: usize) {
        let mut used = 0_usize;
        for table in &mut self.tables {
            let bytes = table.transition_bytes();
            if used + bytes > max_bytes {
                continue;
            }
            table.build_transition_cache();
            used += bytes;
        }
        eprintln!(
            "built dynamic partial transition tables ({:.1} MiB)",
            used as f64 / (1024.0 * 1024.0)
        );
    }
}

fn add_piece_tables(
    tables: &mut Vec<DynamicTable>,
    kind: DynamicKind,
    pieces: &[u8],
    width: usize,
    allowed_moves: &[Move],
) {
    if pieces.is_empty() || tables.len() >= MAX_DYNAMIC_TABLES {
        return;
    }
    if pieces.len() <= width {
        tables.push(DynamicTable::build(kind, pieces.to_vec(), allowed_moves));
        return;
    }
    let stride = width;
    for chunk in pieces.chunks(stride) {
        if tables.len() >= MAX_DYNAMIC_TABLES {
            return;
        }
        if chunk.len() >= 2 {
            tables.push(DynamicTable::build(kind, chunk.to_vec(), allowed_moves));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DynamicKind {
    Corner,
    Edge,
}

struct DynamicTable {
    kind: DynamicKind,
    pieces: Vec<u8>,
    table: Vec<u8>,
    transitions: PieceTransitions,
    transitions_cache: Option<Vec<[u32; MOVE_COUNT]>>,
}

impl DynamicTable {
    fn build(kind: DynamicKind, pieces: Vec<u8>, allowed_moves: &[Move]) -> Self {
        let transitions = PieceTransitions::new();
        let size = match kind {
            DynamicKind::Corner => pow_usize(12, pieces.len()),
            DynamicKind::Edge => pow_usize(12, pieces.len()),
        };
        let mut this = Self {
            kind,
            pieces,
            table: vec![UNVISITED; size],
            transitions,
            transitions_cache: None,
        };
        let solved = this.solved_index();
        this.table[solved] = 0;
        let mut queue = VecDeque::from([solved]);
        while let Some(idx) = queue.pop_front() {
            let depth = this.table[idx];
            for &mv in allowed_moves {
                if let Some(next) = this.move_index(idx, mv) {
                    if this.table[next] == UNVISITED {
                        this.table[next] = depth + 1;
                        queue.push_back(next);
                    }
                }
            }
        }
        this
    }

    fn name(&self) -> String {
        let prefix = match self.kind {
            DynamicKind::Corner => "partial-corner",
            DynamicKind::Edge => "partial-edge",
        };
        format!(
            "{}({}) {:.1} MiB",
            prefix,
            self.pieces
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(","),
            self.table.len() as f64 / (1024.0 * 1024.0)
        )
    }

    fn solved_index(&self) -> usize {
        let mut idx = 0_usize;
        for &piece in &self.pieces {
            let value = match self.kind {
                DynamicKind::Corner => usize::from(piece) * 2,
                DynamicKind::Edge => usize::from(piece),
            };
            idx = idx * 12 + value;
        }
        idx
    }

    fn value(&self, state: &FtoCubie) -> u8 {
        let idx = self.index_of_state(state);
        self.value_for_index(idx)
    }

    fn value_for_index_or_state(&self, idx: usize, state: &FtoCubie) -> u8 {
        if self.transitions_cache.is_some() {
            self.value_for_index(idx)
        } else {
            self.value(state)
        }
    }

    fn value_for_index(&self, idx: usize) -> u8 {
        let value = self.table[idx];
        if value == UNVISITED {
            0
        } else {
            value
        }
    }

    fn move_cached_index(&self, idx: usize, state: &FtoCubie, mv: Move) -> usize {
        self.transitions_cache
            .as_ref()
            .map(|cache| cache[idx][mv.idx()])
            .filter(|&next| next != INVALID_TRANSITION)
            .map_or_else(|| self.index_of_state(state), |next| next as usize)
    }

    fn bytes(&self) -> usize {
        self.table.len() + self.transitions_cache.as_ref().map_or(0, |cache| {
            cache.len() * MOVE_COUNT * std::mem::size_of::<u32>()
        })
    }

    fn transition_bytes(&self) -> usize {
        self.table.len() * MOVE_COUNT * std::mem::size_of::<u32>()
    }

    fn build_transition_cache(&mut self) {
        if self.transitions_cache.is_some() {
            return;
        }
        let mut cache = vec![[INVALID_TRANSITION; MOVE_COUNT]; self.table.len()];
        for (idx, row) in cache.iter_mut().enumerate() {
            if self.table[idx] == UNVISITED {
                continue;
            }
            for &mv in &Move::ALL {
                if let Some(next) = self.move_index(idx, mv) {
                    row[mv.idx()] = next as u32;
                }
            }
        }
        self.transitions_cache = Some(cache);
    }

    fn index_of_state(&self, state: &FtoCubie) -> usize {
        match self.kind {
            DynamicKind::Corner => {
                let mut pos_of = [0_u8; 6];
                let mut ori_of = [0_u8; 6];
                for pos in 0..6 {
                    let piece = state.cp[pos] as usize;
                    pos_of[piece] = pos as u8;
                    ori_of[piece] = state.co[pos];
                }
                self.pieces.iter().fold(0_usize, |idx, &piece| {
                    idx * 12 + usize::from(pos_of[piece as usize] * 2 + ori_of[piece as usize])
                })
            }
            DynamicKind::Edge => {
                let mut pos_of = [0_u8; 12];
                for pos in 0..12 {
                    pos_of[state.ep[pos] as usize] = pos as u8;
                }
                self.pieces
                    .iter()
                    .fold(0_usize, |idx, &piece| idx * 12 + usize::from(pos_of[piece as usize]))
            }
        }
    }

    fn move_index(&self, idx: usize, mv: Move) -> Option<usize> {
        let mut values = vec![0_u8; self.pieces.len()];
        let mut rest = idx;
        for value in values.iter_mut().rev() {
            *value = (rest % 12) as u8;
            rest /= 12;
        }

        match self.kind {
            DynamicKind::Corner => {
                let mut used = [false; 6];
                let mut out = 0_usize;
                for value in values {
                    let pos = value / 2;
                    let ori = value & 1;
                    if used[pos as usize] {
                        return None;
                    }
                    used[pos as usize] = true;
                    let next = self.transitions.corner[mv.idx()][pos as usize];
                    out = out * 12 + usize::from(next.pos * 2 + (ori ^ next.ori));
                }
                Some(out)
            }
            DynamicKind::Edge => {
                let mut used = [false; 12];
                let mut out = 0_usize;
                for pos in values {
                    if used[pos as usize] {
                        return None;
                    }
                    used[pos as usize] = true;
                    out = out * 12 + usize::from(self.transitions.edge[mv.idx()][pos as usize]);
                }
                Some(out)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct CornerNext {
    pos: u8,
    ori: u8,
}

struct PieceTransitions {
    corner: [[CornerNext; 6]; MOVE_COUNT],
    edge: [[u8; 12]; MOVE_COUNT],
}

impl PieceTransitions {
    fn new() -> Self {
        let moves = move_cubies();
        let mut corner = [[CornerNext { pos: 0, ori: 0 }; 6]; MOVE_COUNT];
        let mut edge = [[0_u8; 12]; MOVE_COUNT];
        for (move_idx, mv) in moves.iter().enumerate() {
            for dst in 0..6 {
                let src = mv.cp[dst] as usize;
                corner[move_idx][src] = CornerNext {
                    pos: dst as u8,
                    ori: mv.co[dst],
                };
            }
            for dst in 0..12 {
                edge[move_idx][mv.ep[dst] as usize] = dst as u8;
            }
        }
        Self { corner, edge }
    }
}

fn pow_usize(base: usize, exp: usize) -> usize {
    (0..exp).fold(1, |acc, _| acc * base)
}

fn is_r_face_move(mv: Move) -> bool {
    matches!(mv, Move::R | Move::Rp)
}

fn is_r_trigger(mv: Move) -> bool {
    matches!(mv, Move::RURp | Move::RUpRp | Move::RpUR | Move::RpUpR)
}

fn should_skip_after(commute: &[[bool; MOVE_COUNT]; MOVE_COUNT], last: Move, current: Move) -> bool {
    last.axis() == current.axis()
        || (is_r_face_move(last) && is_r_trigger(current))
        || (is_r_trigger(last) && is_r_face_move(current))
        || (commute[last.idx()][current.idx()] && current.idx() < last.idx())
}

fn move_commutation() -> [[bool; MOVE_COUNT]; MOVE_COUNT] {
    let moves = move_cubies();
    let mut commute = [[false; MOVE_COUNT]; MOVE_COUNT];
    for i in 0..MOVE_COUNT {
        for j in 0..MOVE_COUNT {
            commute[i][j] = moves[i].compose(&moves[j]) == moves[j].compose(&moves[i]);
        }
    }
    commute
}

#[must_use]
pub fn format_partial_solutions(result: SearchResult) -> (u64, Vec<String>) {
    (
        result.nodes,
        result
            .solutions
            .iter()
            .map(|solution| format_solution(solution))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{is_partial_solved, DynamicKind, DynamicTable, PartialMask};
    use crate::{moves::Move, FtoCubie};

    #[test]
    fn cached_edge_transition_matches_rerank() {
        let mut table = DynamicTable::build(DynamicKind::Edge, vec![0, 1, 2], &Move::ALL);
        table.build_transition_cache();
        let state = FtoCubie::solved().apply(Move::R).apply(Move::U);
        let idx = table.index_of_state(&state);
        let next = state.apply(Move::B);
        assert_eq!(table.move_cached_index(idx, &next, Move::B), table.index_of_state(&next));
    }

    #[test]
    fn cached_corner_transition_matches_rerank() {
        let mut table = DynamicTable::build(DynamicKind::Corner, vec![0, 1, 2], &Move::ALL);
        table.build_transition_cache();
        let state = FtoCubie::solved().apply(Move::R).apply(Move::U);
        let idx = table.index_of_state(&state);
        let next = state.apply(Move::B);
        assert_eq!(table.move_cached_index(idx, &next, Move::B), table.index_of_state(&next));
    }

    #[test]
    fn pinned_center_requires_that_visible_center_identity() {
        let mut mask = PartialMask {
            corners: [false; 6],
            edges: [false; 12],
            uf_centers: [false; 12],
            rl_centers: [false; 12],
            uf_center_targets: [Some(1), None, None, None],
            rl_center_targets: [None; 4],
            last_layer_centers: false,
        };
        mask.uf_centers[0] = true;

        let mut wrong_duplicate_in_target = FtoCubie::solved();
        wrong_duplicate_in_target.uf.swap(1, 2);
        assert!(!is_partial_solved(&wrong_duplicate_in_target, &mask));

        let mut pinned_identity_in_target = FtoCubie::solved();
        pinned_identity_in_target.uf.swap(0, 1);
        assert!(is_partial_solved(&pinned_identity_in_target, &mask));
    }

    #[test]
    fn last_layer_rl_centers_keep_fixed_duplicate_separate() {
        let mut mask = PartialMask {
            corners: [false; 6],
            edges: [false; 12],
            uf_centers: [false; 12],
            rl_centers: [false; 12],
            uf_center_targets: [None; 4],
            rl_center_targets: [None; 4],
            last_layer_centers: true,
        };
        mask.rl_centers[3] = true;
        mask.rl_centers[4] = true;

        let solved = FtoCubie::solved();
        assert!(is_partial_solved(&solved, &mask));

        let mut affected_swap = FtoCubie::solved();
        affected_swap.rl.swap(4, 5);
        assert!(is_partial_solved(&affected_swap, &mask));

        let mut fixed_swap = FtoCubie::solved();
        fixed_swap.rl.swap(3, 4);
        assert!(!is_partial_solved(&fixed_swap, &mask));
    }
}
