use crate::{
    coord::EdgeCoord,
    moves::{move_cubies, Move, MOVE_COUNT},
    pruning::SolverPruning,
    tables::TransitionTables,
    FtoCubie,
    FtoCoord,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

#[derive(Clone, Debug)]
pub struct SearchConfig {
    pub min_depth: u8,
    pub max_depth: u8,
    pub find_all: bool,
    pub allowed_moves: Vec<Move>,
    pub free_u_ends: bool,
    pub cancel: Option<Arc<AtomicBool>>,
}

pub type DepthReporter<'a> = dyn Fn(u8) + Send + Sync + 'a;

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            min_depth: 0,
            max_depth: 6,
            find_all: true,
            allowed_moves: Move::ALL.to_vec(),
            free_u_ends: false,
            cancel: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub solutions: Vec<Vec<Move>>,
    pub nodes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BidirectionalConfig {
    pub depth: u8,
    pub find_all: bool,
    pub max_stored_paths: usize,
    pub threads: usize,
    pub use_start_pruning: bool,
    pub progress_interval: usize,
    pub allowed_moves: Vec<Move>,
}

#[must_use]
pub fn solve(coord: FtoCoord, tables: &TransitionTables, config: &SearchConfig) -> SearchResult {
    solve_with_pruning(coord, tables, None, config)
}

#[must_use]
pub fn solve_with_pruning(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
) -> SearchResult {
    solve_with_pruning_threads(coord, tables, pruning, config, 1)
}

#[must_use]
pub fn solve_with_pruning_threads(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
    threads: usize,
) -> SearchResult {
    solve_with_pruning_threads_impl(coord, tables, pruning, config, threads, None)
}

#[must_use]
pub fn solve_with_pruning_threads_reporting(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
    threads: usize,
    report: impl Fn(u8) + Send + Sync,
) -> SearchResult {
    solve_with_pruning_threads_impl(coord, tables, pruning, config, threads, Some(&report))
}

#[must_use]
pub fn solve_last_layer_with_pruning_threads(
    cubie: FtoCubie,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
    threads: usize,
) -> SearchResult {
    let coord = cubie.coord();
    let start_depth = config.min_depth;
    let config = SearchConfig {
        min_depth: start_depth,
        max_depth: config.max_depth,
        find_all: config.find_all,
        allowed_moves: config.allowed_moves.clone(),
        free_u_ends: true,
        cancel: config.cancel.clone(),
    };
    solve_last_layer_impl(cubie, SearchState::from_coord(coord), tables, pruning, &config, threads)
}

fn solve_last_layer_impl(
    cubie: FtoCubie,
    state: SearchState,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
    threads: usize,
) -> SearchResult {
    if threads <= 1 || config.max_depth <= 1 {
        return solve_last_layer_single(cubie, state, tables, pruning, config);
    }

    let moves = move_cubies();
    let commute = move_commutation();
    let mut total = SearchResult {
        solutions: Vec::new(),
        nodes: 0,
    };

    for depth in config.min_depth..=config.max_depth {
        if is_cancelled(config) {
            break;
        }
        if depth == 0 {
            let mut ctx = LastLayerSearchContext::new(tables, pruning, config);
            for (prefix, start_cubie, start_state, _) in last_layer_start_states(cubie, state, tables, &moves) {
                ctx.nodes += 1;
                if let Some(suffix) = last_layer_free_u_suffix(start_cubie, &moves) {
                    let mut solution = Vec::new();
                    if let Some(prefix) = prefix {
                        solution.push(prefix);
                    }
                    if let Some(suffix) = suffix {
                        solution.push(suffix);
                    }
                    ctx.solutions.push(solution);
                    if !config.find_all {
                        break;
                    }
                }
                let _ = start_state;
            }
            total.nodes += ctx.nodes;
            total.solutions.extend(ctx.solutions);
            if !config.find_all && !total.solutions.is_empty() {
                break;
            }
            continue;
        }

        let child_depth = depth - 1;
        let mut roots = Vec::new();
        for (prefix, start_cubie, start_state, last_move) in
            last_layer_start_states(cubie, state, tables, &moves)
        {
            for &mv in &config.allowed_moves {
                if last_move.is_some_and(|last| should_skip_after(&commute, last, mv)) {
                    continue;
                }
                let pruning_child = start_state.apply_pruning(tables, mv);
                let pruning_value = adjusted_pruning_value(
                    pruning
                        .map(|pdb| {
                            pdb.heuristic(pruning_child.edge3_uf3_idx, pruning_child.corner_uf3_idx)
                        })
                        .unwrap_or(0),
                    true,
                );
                if pruning_value > child_depth {
                    continue;
                }
                roots.push((
                    prefix,
                    mv,
                    start_cubie.compose(&moves[mv.idx()]),
                    start_state.apply_with_pruning_child(tables, mv, pruning_child),
                ));
            }
        }

        if roots.is_empty() {
            total.nodes += 1;
            continue;
        }

        let worker_count = threads.min(roots.len());
        let chunk_size = roots.len().div_ceil(worker_count);
        let mut depth_result = SearchResult {
            solutions: Vec::new(),
            nodes: 1,
        };

        thread::scope(|scope| {
            let mut handles = Vec::new();
            for chunk in roots.chunks(chunk_size) {
                handles.push(scope.spawn(move || {
                    let mut ctx = LastLayerSearchContext::new(tables, pruning, config);
                    for &(prefix, mv, next_cubie, next_state) in chunk {
                        if let Some(prefix) = prefix {
                            ctx.path.push(prefix);
                        }
                        ctx.path.push(mv);
                        ctx.dfs(next_cubie, next_state, child_depth, Some(mv), true);
                        ctx.path.pop();
                        if prefix.is_some() {
                            ctx.path.pop();
                        }
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
                let result = handle.join().expect("last-layer search worker panicked");
                depth_result.nodes += result.nodes;
                depth_result.solutions.extend(result.solutions);
            }
        });

        total.nodes += depth_result.nodes;
        total.solutions.extend(depth_result.solutions);
        if !config.find_all && !total.solutions.is_empty() {
            break;
        }
    }
    total
}

fn solve_last_layer_single(
    cubie: FtoCubie,
    state: SearchState,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
) -> SearchResult {
    let moves = move_cubies();
    let mut ctx = LastLayerSearchContext::new(tables, pruning, config);
    for depth in config.min_depth..=config.max_depth {
        if is_cancelled(config) {
            break;
        }
        for (prefix, start_cubie, start_state, last_move) in
            last_layer_start_states(cubie, state, tables, &moves)
        {
            if let Some(prefix) = prefix {
                ctx.path.push(prefix);
            }
            ctx.dfs(start_cubie, start_state, depth, last_move, false);
            if prefix.is_some() {
                ctx.path.pop();
            }
            if !config.find_all && !ctx.solutions.is_empty() {
                break;
            }
        }
        if !config.find_all && !ctx.solutions.is_empty() {
            break;
        }
    }
    SearchResult {
        solutions: ctx.solutions,
        nodes: ctx.nodes,
    }
}

fn solve_with_pruning_threads_impl(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
    threads: usize,
    report: Option<&DepthReporter<'_>>,
) -> SearchResult {
    if threads <= 1 || config.max_depth <= 1 {
        return solve_with_pruning_single(coord, tables, pruning, config, report);
    }

    let root = SearchState::from_coord(coord);
    let solved = SearchState::from_coord(FtoCoord::solved());
    let solved_u = SearchState::from_coord(FtoCubie::solved().apply(Move::U).coord());
    let solved_up = SearchState::from_coord(FtoCubie::solved().apply(Move::Up).coord());
    let commute = move_commutation();
    let mut total = SearchResult {
        solutions: Vec::new(),
        nodes: 0,
    };

    for depth in config.min_depth..=config.max_depth {
        if is_cancelled(config) {
            break;
        }
        if let Some(report) = report {
            report(depth);
        }
        if depth == 0 {
            for (prefix, start, _) in exact_start_states(root, tables, config.free_u_ends) {
                total.nodes += 1;
                if let Some(suffix) =
                    free_u_suffix(start, solved, solved_u, solved_up, config.free_u_ends)
                {
                    let mut solution = Vec::new();
                    if let Some(prefix) = prefix {
                        solution.push(prefix);
                    }
                    if let Some(suffix) = suffix {
                        solution.push(suffix);
                    }
                    total.solutions.push(solution);
                    if !config.find_all {
                        break;
                    }
                }
            }
            if !config.find_all && !total.solutions.is_empty() {
                break;
            }
            continue;
        }

        let child_depth = depth - 1;
        let mut roots = Vec::new();
        for (prefix, start, last_move) in exact_start_states(root, tables, config.free_u_ends) {
            for &mv in &config.allowed_moves {
                if last_move.is_some_and(|last| should_skip_after(&commute, last, mv)) {
                    continue;
                }
                let pruning_child = start.apply_pruning(tables, mv);
                let pruning_value = adjusted_pruning_value(
                    pruning
                        .map(|pdb| {
                            pdb.heuristic(pruning_child.edge3_uf3_idx, pruning_child.corner_uf3_idx)
                        })
                        .unwrap_or(0),
                    config.free_u_ends,
                );
                if pruning_value > child_depth {
                    continue;
                }
                let state = start.apply_with_pruning_child(tables, mv, pruning_child);
                roots.push((prefix, mv, state));
            }
        }
        if roots.is_empty() {
            total.nodes += 1;
            continue;
        }

        let worker_count = threads.min(roots.len());
        let chunk_size = roots.len().div_ceil(worker_count);
        let mut depth_result = SearchResult {
            solutions: Vec::new(),
            nodes: 1,
        };

        thread::scope(|scope| {
            let mut handles = Vec::new();
            for chunk in roots.chunks(chunk_size) {
                handles.push(scope.spawn(move || {
                    let mut ctx = SearchContext {
                        tables,
                        pruning,
                        commute,
                        config,
                        solved,
                        solved_u,
                        solved_up,
                        solutions: Vec::new(),
                        path: Vec::with_capacity(config.max_depth as usize),
                        nodes: 0,
                    };
                    for &(prefix, mv, state) in chunk {
                        if is_cancelled(config) {
                            break;
                        }
                        if let Some(prefix) = prefix {
                            ctx.path.push(prefix);
                        }
                        ctx.path.push(mv);
                        ctx.dfs(state, child_depth, Some(mv), true);
                        ctx.path.pop();
                        if prefix.is_some() {
                            ctx.path.pop();
                        }
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
                let result = handle.join().expect("search worker panicked");
                depth_result.nodes += result.nodes;
                depth_result.solutions.extend(result.solutions);
            }
        });

        total.nodes += depth_result.nodes;
        total.solutions.extend(depth_result.solutions);
        if !config.find_all && !total.solutions.is_empty() {
            break;
        }
    }

    total
}

pub fn solve_bidirectional(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &BidirectionalConfig,
) -> Result<SearchResult, String> {
    let fwd_depth = config.depth / 2;
    let back_depth = config.depth - fwd_depth;
    let commute = move_commutation();
    let solved = SearchState::from_coord(FtoCoord::solved());
    let start = SearchState::from_coord(coord);
    let allowed_moves = config.allowed_moves.as_slice();
    let start_pruning = if config.use_start_pruning {
        eprintln!("building temporary start-centered bidirectional pruning tables...");
        Some(SolverPruning::build_from_coord_with_moves(
            coord,
            tables,
            config.progress_interval,
            allowed_moves,
        )?)
    } else {
        None
    };

    let mut back = BackwardBuilder {
        tables,
        pruning: start_pruning.as_ref(),
        commute,
        allowed_moves,
        goal_slack: fwd_depth,
        max_stored_paths: config.max_stored_paths,
        stored_paths: 0,
        nodes: 0,
        paths: HashMap::new(),
    };
    back.build(solved, back_depth, None, 0)?;

    if config.threads > 1 && fwd_depth > 1 {
        return Ok(match_bidirectional_parallel(
            start,
            fwd_depth,
            back_depth,
            tables,
            pruning,
            &back.paths,
            config.find_all,
            back.nodes,
            commute,
            config.threads,
            allowed_moves,
        ));
    }

    let mut matcher = ForwardMatcher {
        tables,
        pruning,
        commute,
        allowed_moves,
        goal_slack: back_depth,
        back_paths: &back.paths,
        find_all: config.find_all,
        solutions: Vec::new(),
        nodes: back.nodes,
        path: Vec::with_capacity(config.depth as usize),
    };
    matcher.search(start, fwd_depth, None);

    Ok(SearchResult {
        solutions: matcher.solutions,
        nodes: matcher.nodes,
    })
}

fn match_bidirectional_parallel(
    start: SearchState,
    fwd_depth: u8,
    back_depth: u8,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    back_paths: &HashMap<SearchState, Vec<u64>>,
    find_all: bool,
    initial_nodes: u64,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    threads: usize,
    allowed_moves: &[Move],
) -> SearchResult {
    let child_depth = fwd_depth - 1;
    let mut roots = Vec::new();
    for &mv in allowed_moves {
        let pruning_child = start.apply_pruning(tables, mv);
        roots.push((mv, start.apply_with_pruning_child(tables, mv, pruning_child)));
    }
    if roots.is_empty() {
        return SearchResult {
            solutions: Vec::new(),
            nodes: initial_nodes + 1,
        };
    }

    let worker_count = threads.min(roots.len());
    let chunk_size = roots.len().div_ceil(worker_count);
    let mut total = SearchResult {
        solutions: Vec::new(),
        nodes: initial_nodes + 1,
    };
    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in roots.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                let mut matcher = ForwardMatcher {
                    tables,
                    pruning,
                    commute,
                    allowed_moves,
                    goal_slack: back_depth,
                    back_paths,
                    find_all,
                    solutions: Vec::new(),
                    nodes: 0,
                    path: Vec::with_capacity(fwd_depth as usize),
                };
                for &(mv, state) in chunk {
                    matcher.path.push(mv);
                    matcher.search(state, child_depth, Some(mv));
                    matcher.path.pop();
                    if !find_all && !matcher.solutions.is_empty() {
                        break;
                    }
                }
                SearchResult {
                    solutions: matcher.solutions,
                    nodes: matcher.nodes,
                }
            }));
        }

        for handle in handles {
            let result = handle.join().expect("bidirectional worker panicked");
            total.nodes += result.nodes;
            total.solutions.extend(result.solutions);
        }
    });
    total
}

fn solve_with_pruning_single(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
    report: Option<&DepthReporter<'_>>,
) -> SearchResult {
    let mut ctx = SearchContext {
        tables,
        pruning,
        commute: move_commutation(),
        config,
        solved: SearchState::from_coord(FtoCoord::solved()),
        solved_u: SearchState::from_coord(FtoCubie::solved().apply(Move::U).coord()),
        solved_up: SearchState::from_coord(FtoCubie::solved().apply(Move::Up).coord()),
        solutions: Vec::new(),
        path: Vec::with_capacity(config.max_depth as usize),
        nodes: 0,
    };
    let state = SearchState::from_coord(coord);

    for depth in config.min_depth..=config.max_depth {
        if is_cancelled(config) {
            break;
        }
        if let Some(report) = report {
            report(depth);
        }
        for (prefix, start, last_move) in exact_start_states(state, tables, config.free_u_ends) {
            if let Some(prefix) = prefix {
                ctx.path.push(prefix);
            }
            ctx.dfs(start, depth, last_move, false);
            if prefix.is_some() {
                ctx.path.pop();
            }
            if !config.find_all && !ctx.solutions.is_empty() {
                break;
            }
        }
        if !config.find_all && !ctx.solutions.is_empty() {
            break;
        }
    }

    SearchResult {
        solutions: ctx.solutions,
        nodes: ctx.nodes,
    }
}

struct SearchContext<'a> {
    tables: &'a TransitionTables,
    pruning: Option<&'a SolverPruning>,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    config: &'a SearchConfig,
    solved: SearchState,
    solved_u: SearchState,
    solved_up: SearchState,
    solutions: Vec<Vec<Move>>,
    path: Vec<Move>,
    nodes: u64,
}

struct LastLayerSearchContext<'a> {
    tables: &'a TransitionTables,
    pruning: Option<&'a SolverPruning>,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    moves: [FtoCubie; MOVE_COUNT],
    config: &'a SearchConfig,
    solutions: Vec<Vec<Move>>,
    path: Vec<Move>,
    nodes: u64,
}

impl<'a> LastLayerSearchContext<'a> {
    fn new(
        tables: &'a TransitionTables,
        pruning: Option<&'a SolverPruning>,
        config: &'a SearchConfig,
    ) -> Self {
        Self {
            tables,
            pruning,
            commute: move_commutation(),
            moves: move_cubies(),
            config,
            solutions: Vec::new(),
            path: Vec::with_capacity(config.max_depth as usize),
            nodes: 0,
        }
    }

    fn dfs(
        &mut self,
        cubie: FtoCubie,
        state: SearchState,
        depth_left: u8,
        last_move: Option<Move>,
        pruning_checked: bool,
    ) {
        if is_cancelled(self.config) {
            return;
        }
        self.nodes += 1;
        if !pruning_checked && self.pruning_value(state) > depth_left {
            return;
        }
        if depth_left == 0 {
            if ends_in_u_or_up(&self.path) {
                return;
            }
            if let Some(suffix) = last_layer_free_u_suffix(cubie, &self.moves) {
                let mut solution = self.path.clone();
                if let Some(suffix) = suffix {
                    solution.push(suffix);
                }
                self.solutions.push(solution);
            }
            return;
        }

        let child_depth = depth_left - 1;
        let mut children = [(0_u8, Move::U, cubie, state); MOVE_COUNT];
        let mut child_count = 0;
        for &mv in &self.config.allowed_moves {
            if last_move.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            let pruning_child = state.apply_pruning(self.tables, mv);
            let pruning_value = self.pruning_value_for_child(pruning_child);
            if pruning_value > child_depth {
                continue;
            }
            children[child_count] = (
                pruning_value,
                mv,
                cubie.compose(&self.moves[mv.idx()]),
                state.apply_with_pruning_child(self.tables, mv, pruning_child),
            );
            child_count += 1;
        }
        if !self.config.find_all {
            children[..child_count].sort_unstable_by(|a, b| b.0.cmp(&a.0));
        }

        for &(_, mv, next_cubie, next_state) in &children[..child_count] {
            self.path.push(mv);
            self.dfs(next_cubie, next_state, child_depth, Some(mv), true);
            self.path.pop();
            if !self.config.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }

    fn pruning_value(&self, state: SearchState) -> u8 {
        adjusted_pruning_value(
            self.pruning
                .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
                .unwrap_or(0),
            true,
        )
    }

    fn pruning_value_for_child(&self, child: PruningChild) -> u8 {
        adjusted_pruning_value(
            self.pruning
                .map(|pruning| pruning.heuristic(child.edge3_uf3_idx, child.corner_uf3_idx))
                .unwrap_or(0),
            true,
        )
    }
}

impl SearchContext<'_> {
    fn dfs(
        &mut self,
        state: SearchState,
        depth_left: u8,
        last_move: Option<Move>,
        pruning_checked: bool,
    ) {
        if is_cancelled(self.config) {
            return;
        }
        self.nodes += 1;
        if !pruning_checked && self.pruning_value(state) > depth_left {
            return;
        }
        if depth_left == 0 {
            if let Some(suffix) = free_u_suffix(
                state,
                self.solved,
                self.solved_u,
                self.solved_up,
                self.config.free_u_ends,
            ) {
                let mut solution = self.path.clone();
                if let Some(suffix) = suffix {
                    solution.push(suffix);
                }
                self.solutions.push(solution);
            }
            return;
        }

        let child_depth = depth_left - 1;
        let mut children = [(0_u8, Move::U, state); MOVE_COUNT];
        let mut child_count = 0;
        for &mv in &self.config.allowed_moves {
            if last_move.is_some_and(|last| self.should_skip_after(last, mv)) {
                continue;
            }
            let pruning_child = state.apply_pruning(self.tables, mv);
            let pruning_value = self.pruning_value_for_child(pruning_child);
            if pruning_value > child_depth {
                continue;
            }
            let next = state.apply_with_pruning_child(self.tables, mv, pruning_child);
            children[child_count] = (pruning_value, mv, next);
            child_count += 1;
        }
        if !self.config.find_all {
            children[..child_count].sort_unstable_by(|a, b| b.0.cmp(&a.0));
        }

        for &(_, mv, next) in &children[..child_count] {
            if is_cancelled(self.config) {
                return;
            }
            self.path.push(mv);
            self.dfs(next, child_depth, Some(mv), true);
            self.path.pop();

            if !self.config.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }

    fn pruning_value(&self, state: SearchState) -> u8 {
        adjusted_pruning_value(
            self.pruning
            .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
                .unwrap_or(0),
            self.config.free_u_ends,
        )
    }

    fn pruning_value_for_child(&self, child: PruningChild) -> u8 {
        adjusted_pruning_value(
            self.pruning
            .map(|pruning| pruning.heuristic(child.edge3_uf3_idx, child.corner_uf3_idx))
                .unwrap_or(0),
            self.config.free_u_ends,
        )
    }

    fn should_skip_after(&self, last: Move, current: Move) -> bool {
        should_skip_after(&self.commute, last, current)
    }
}

fn is_cancelled(config: &SearchConfig) -> bool {
    config
        .cancel
        .as_ref()
        .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

fn ends_in_u_or_up(path: &[Move]) -> bool {
    matches!(path.last(), Some(&Move::U) | Some(&Move::Up))
}

fn adjusted_pruning_value(value: u8, free_u_ends: bool) -> u8 {
    if free_u_ends {
        value.saturating_sub(1)
    } else {
        value
    }
}

fn exact_start_states(
    root: SearchState,
    tables: &TransitionTables,
    free_u_ends: bool,
) -> Vec<(Option<Move>, SearchState, Option<Move>)> {
    if !free_u_ends {
        return vec![(None, root, None)];
    }
    let u_child = root.apply_pruning(tables, Move::U);
    let up_child = root.apply_pruning(tables, Move::Up);
    vec![
        (None, root, None),
        (
            Some(Move::U),
            root.apply_with_pruning_child(tables, Move::U, u_child),
            Some(Move::U),
        ),
        (
            Some(Move::Up),
            root.apply_with_pruning_child(tables, Move::Up, up_child),
            Some(Move::Up),
        ),
    ]
}

fn free_u_suffix(
    state: SearchState,
    solved: SearchState,
    solved_u: SearchState,
    solved_up: SearchState,
    free_u_ends: bool,
) -> Option<Option<Move>> {
    if state == solved {
        return Some(None);
    }
    if !free_u_ends {
        return None;
    }
    if state == solved_u {
        return Some(Some(Move::Up));
    }
    if state == solved_up {
        return Some(Some(Move::U));
    }
    None
}

fn last_layer_start_states(
    root_cubie: FtoCubie,
    root_state: SearchState,
    tables: &TransitionTables,
    moves: &[FtoCubie; MOVE_COUNT],
) -> Vec<(Option<Move>, FtoCubie, SearchState, Option<Move>)> {
    let u_child = root_state.apply_pruning(tables, Move::U);
    let up_child = root_state.apply_pruning(tables, Move::Up);
    vec![
        (None, root_cubie, root_state, None),
        (
            Some(Move::U),
            root_cubie.compose(&moves[Move::U.idx()]),
            root_state.apply_with_pruning_child(tables, Move::U, u_child),
            Some(Move::U),
        ),
        (
            Some(Move::Up),
            root_cubie.compose(&moves[Move::Up.idx()]),
            root_state.apply_with_pruning_child(tables, Move::Up, up_child),
            Some(Move::Up),
        ),
    ]
}

fn last_layer_free_u_suffix(
    cubie: FtoCubie,
    moves: &[FtoCubie; MOVE_COUNT],
) -> Option<Option<Move>> {
    if is_last_layer_solved(&cubie) {
        return Some(None);
    }
    if is_last_layer_solved(&cubie.compose(&moves[Move::U.idx()])) {
        return Some(Some(Move::U));
    }
    if is_last_layer_solved(&cubie.compose(&moves[Move::Up.idx()])) {
        return Some(Some(Move::Up));
    }
    None
}

fn is_last_layer_solved(cubie: &FtoCubie) -> bool {
    if cubie.cp != [0, 1, 2, 3, 4, 5] || cubie.co != [0; 6] {
        return false;
    }
    if cubie.ep != [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
        return false;
    }
    for pos in 0..12 {
        if cubie.uf[pos] / 3 != pos as u8 / 3 {
            return false;
        }
        if !rl_last_layer_center_solved(cubie.rl[pos], pos as u8) {
            return false;
        }
    }
    true
}

fn rl_last_layer_center_solved(piece: u8, pos: u8) -> bool {
    const FIXED: [Option<u8>; 4] = [None, Some(3), Some(8), Some(10)];
    let color = piece / 3;
    let pos_color = pos / 3;
    if color != pos_color {
        return false;
    }
    if let Some(fixed_slot) = FIXED[color as usize] {
        if piece == fixed_slot {
            pos == fixed_slot
        } else {
            pos != fixed_slot
        }
    } else {
        true
    }
}

struct BackwardBuilder<'a> {
    tables: &'a TransitionTables,
    pruning: Option<&'a SolverPruning>,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    allowed_moves: &'a [Move],
    goal_slack: u8,
    max_stored_paths: usize,
    stored_paths: usize,
    nodes: u64,
    paths: HashMap<SearchState, Vec<u64>>,
}

impl BackwardBuilder<'_> {
    fn build(
        &mut self,
        state: SearchState,
        depth_left: u8,
        last_move: Option<Move>,
        encoded_path: u64,
    ) -> Result<(), String> {
        self.nodes += 1;
        if self.pruning_value(state) > self.goal_slack + depth_left {
            return Ok(());
        }
        if depth_left == 0 {
            self.paths.entry(state).or_default().push(encoded_path);
            self.stored_paths += 1;
            if self.stored_paths > self.max_stored_paths {
                return Err(format!(
                    "bidirectional table exceeded cap of {} stored paths",
                    self.max_stored_paths
                ));
            }
            return Ok(());
        }

        for &mv in self.allowed_moves {
            if last_move.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            let pruning_child = state.apply_pruning(self.tables, mv);
            if self.pruning_value_for_child(pruning_child) > self.goal_slack + depth_left - 1 {
                continue;
            }
            let next = state.apply_with_pruning_child(self.tables, mv, pruning_child);
            self.build(
                next,
                depth_left - 1,
                Some(mv),
                encoded_path * MOVE_COUNT as u64 + mv.idx() as u64 + 1,
            )?;
        }
        Ok(())
    }

    fn pruning_value(&self, state: SearchState) -> u8 {
        self.pruning
            .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
            .unwrap_or(0)
    }

    fn pruning_value_for_child(&self, child: PruningChild) -> u8 {
        self.pruning
            .map(|pruning| pruning.heuristic(child.edge3_uf3_idx, child.corner_uf3_idx))
            .unwrap_or(0)
    }
}

struct ForwardMatcher<'a> {
    tables: &'a TransitionTables,
    pruning: Option<&'a SolverPruning>,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    allowed_moves: &'a [Move],
    goal_slack: u8,
    back_paths: &'a HashMap<SearchState, Vec<u64>>,
    find_all: bool,
    solutions: Vec<Vec<Move>>,
    nodes: u64,
    path: Vec<Move>,
}

impl ForwardMatcher<'_> {
    fn search(&mut self, state: SearchState, depth_left: u8, last_move: Option<Move>) {
        self.nodes += 1;
        if self.pruning_value(state) > self.goal_slack + depth_left {
            return;
        }
        if depth_left == 0 {
            if let Some(back_paths) = self.back_paths.get(&state) {
                for &encoded_back in back_paths {
                    let suffix = decode_inverse_path(encoded_back);
                    if let (Some(&last), Some(&first)) = (self.path.last(), suffix.first()) {
                        if should_skip_after(&self.commute, last, first) {
                            continue;
                        }
                    }
                    let mut solution = self.path.clone();
                    solution.extend(suffix);
                    self.solutions.push(solution);
                    if !self.find_all {
                        return;
                    }
                }
            }
            return;
        }

        for &mv in self.allowed_moves {
            if last_move.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            let pruning_child = state.apply_pruning(self.tables, mv);
            if self.pruning_value_for_child(pruning_child) > self.goal_slack + depth_left - 1 {
                continue;
            }
            let next = state.apply_with_pruning_child(self.tables, mv, pruning_child);
            self.path.push(mv);
            self.search(next, depth_left - 1, Some(mv));
            self.path.pop();
            if !self.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }

    fn pruning_value(&self, state: SearchState) -> u8 {
        self.pruning
            .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
            .unwrap_or(0)
    }

    fn pruning_value_for_child(&self, child: PruningChild) -> u8 {
        self.pruning
            .map(|pruning| pruning.heuristic(child.edge3_uf3_idx, child.corner_uf3_idx))
            .unwrap_or(0)
    }
}

fn decode_inverse_path(mut encoded: u64) -> Vec<Move> {
    let mut moves = Vec::new();
    while encoded > 0 {
        let stored = ((encoded - 1) % MOVE_COUNT as u64) as usize;
        moves.push(Move::from_idx(stored).inverse());
        encoded = (encoded - 1) / MOVE_COUNT as u64;
    }
    moves
}

fn should_skip_after(commute: &[[bool; MOVE_COUNT]; MOVE_COUNT], last: Move, current: Move) -> bool {
    last.axis() == current.axis() || (commute[last.idx()][current.idx()] && current.idx() < last.idx())
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SearchState {
    corner: u16,
    edge: EdgeCoord,
    edge3: u16,
    uf_center: u32,
    uf3: u16,
    rl_center: u32,
    edge3_uf3_idx: usize,
    corner_uf3_idx: usize,
}

impl SearchState {
    fn from_coord(coord: FtoCoord) -> Self {
        Self {
            corner: coord.corner,
            edge: coord.edge,
            edge3: coord.edge3,
            uf_center: coord.uf_center,
            uf3: coord.uf_center3,
            rl_center: coord.rl_center,
            edge3_uf3_idx: SolverPruning::edge3_uf3_index(coord.edge3, coord.uf_center3),
            corner_uf3_idx: SolverPruning::corner_uf3_index(coord.corner, coord.uf_center3),
        }
    }

    fn apply_pruning(self, tables: &TransitionTables, mv: Move) -> PruningChild {
        let corner = tables.corner_move(self.corner, mv);
        let edge3 = tables.edge3_move(self.edge3, mv);
        let uf3 = tables.uf_center3_move(self.uf3, mv);
        PruningChild {
            corner,
            edge3,
            uf3,
            edge3_uf3_idx: SolverPruning::edge3_uf3_index(edge3, uf3),
            corner_uf3_idx: SolverPruning::corner_uf3_index(corner, uf3),
        }
    }

    fn apply_with_pruning_child(
        self,
        tables: &TransitionTables,
        mv: Move,
        pruning_child: PruningChild,
    ) -> Self {
        Self {
            corner: pruning_child.corner,
            edge: EdgeCoord {
                e0: tables.edge_choice_move(self.edge.e0, mv),
                e1: tables.edge_choice_move(self.edge.e1, mv),
                e2: tables.edge_choice_move(self.edge.e2, mv),
                e3: tables.edge_choice_move(self.edge.e3, mv),
            },
            edge3: pruning_child.edge3,
            uf_center: tables.uf_center_move(self.uf_center, mv),
            uf3: pruning_child.uf3,
            rl_center: tables.rl_center_move(self.rl_center, mv),
            edge3_uf3_idx: pruning_child.edge3_uf3_idx,
            corner_uf3_idx: pruning_child.corner_uf3_idx,
        }
    }
}

#[derive(Clone, Copy)]
struct PruningChild {
    corner: u16,
    edge3: u16,
    uf3: u16,
    edge3_uf3_idx: usize,
    corner_uf3_idx: usize,
}

#[must_use]
pub fn format_solution(solution: &[Move]) -> String {
    solution.iter().map(|mv| mv.name()).collect::<Vec<_>>().join(" ")
}

pub const RAW_BRANCHING_FACTOR: usize = MOVE_COUNT;

#[cfg(test)]
mod tests {
    use crate::{moves::Move, search::SearchConfig, tables::TransitionTables, FtoCubie};

    use super::{format_solution, solve};

    #[test]
    #[ignore = "builds full center transition tables"]
    fn finds_inverse_of_short_scramble() {
        let tables = TransitionTables::build();
        let scrambled = FtoCubie::solved().apply(Move::R).apply(Move::U);
        let result = solve(
            scrambled.coord(),
            &tables,
            &SearchConfig {
                min_depth: 0,
                max_depth: 2,
                find_all: false,
                allowed_moves: Move::ALL.to_vec(),
                free_u_ends: false,
                cancel: None,
            },
        );

        assert_eq!(format_solution(&result.solutions[0]), "U' R'");
    }
}
