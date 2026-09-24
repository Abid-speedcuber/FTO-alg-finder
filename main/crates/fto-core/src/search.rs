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
    hash::{Hash, Hasher},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

#[derive(Clone)]
pub struct SearchConfig {
    pub min_depth: u8,
    pub max_depth: u8,
    pub find_all: bool,
    pub allowed_moves: Vec<Move>,
    pub free_u_ends: bool,
    pub cancel: Option<Arc<AtomicBool>>,
    pub solution_reporter: Option<Arc<SolutionReporter>>,
}

pub type DepthReporter<'a> = dyn Fn(u8) + Send + Sync + 'a;
pub type SolutionReporter = dyn Fn(&[Move]) + Send + Sync;

impl std::fmt::Debug for SearchConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchConfig")
            .field("min_depth", &self.min_depth)
            .field("max_depth", &self.max_depth)
            .field("find_all", &self.find_all)
            .field("allowed_moves", &self.allowed_moves)
            .field("free_u_ends", &self.free_u_ends)
            .field("cancel", &self.cancel.is_some())
            .field("solution_reporter", &self.solution_reporter.is_some())
            .finish()
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            min_depth: 0,
            max_depth: 6,
            find_all: true,
            allowed_moves: Move::ALL.to_vec(),
            free_u_ends: false,
            cancel: None,
            solution_reporter: None,
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

const MAX_MINI_TABLES: usize = 8;
const INVALID_MINI_TRANSITION: u32 = u32::MAX;

#[derive(Clone, Debug)]
pub struct MiniPruning {
    tables: Vec<MiniTable>,
}

impl MiniPruning {
    pub fn build_restricted(tables: &TransitionTables, moves: &[Move]) -> Self {
        let specs = [
            MiniSpec::corner(),
            MiniSpec::edge_choice(0),
            MiniSpec::edge_choice(1),
            MiniSpec::edge_choice(2),
            MiniSpec::edge_choice(3),
            MiniSpec::edge3(),
            MiniSpec::uf3(),
        ];
        let tables = specs
            .into_iter()
            .map(|spec| MiniTable::build(spec, tables, moves))
            .collect();
        Self { tables }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.tables.iter().map(MiniTable::bytes).sum()
    }

    #[must_use]
    fn heuristic(&self, indices: &[usize; MAX_MINI_TABLES]) -> u8 {
        self.tables
            .iter()
            .enumerate()
            .map(|(idx, table)| table.value(indices[idx]))
            .max()
            .unwrap_or(0)
    }

    fn indices_of_state(&self, state: SearchState) -> [usize; MAX_MINI_TABLES] {
        let mut indices = [0_usize; MAX_MINI_TABLES];
        for (idx, table) in self.tables.iter().enumerate() {
            indices[idx] = table.spec.index_of_state(state);
        }
        indices
    }

    fn move_indices(
        &self,
        indices: &[usize; MAX_MINI_TABLES],
        mv: Move,
    ) -> [usize; MAX_MINI_TABLES] {
        let mut next = [0_usize; MAX_MINI_TABLES];
        for (idx, table) in self.tables.iter().enumerate() {
            next[idx] = table.next_index(indices[idx], mv);
        }
        next
    }
}

#[derive(Clone, Debug)]
struct MiniTable {
    spec: MiniSpec,
    table: Vec<u8>,
    transitions: Vec<[u32; MOVE_COUNT]>,
}

impl MiniTable {
    fn build(spec: MiniSpec, tables: &TransitionTables, moves: &[Move]) -> Self {
        let size = spec.size();
        let mut table = vec![u8::MAX; size];
        let mut transitions = vec![[INVALID_MINI_TRANSITION; MOVE_COUNT]; size];
        let solved = spec.solved_index();
        table[solved] = 0;
        let mut queue = std::collections::VecDeque::from([solved]);
        while let Some(idx) = queue.pop_front() {
            let depth = table[idx];
            for &mv in moves {
                let next = spec.next_index(idx, mv, tables);
                transitions[idx][mv.idx()] = next as u32;
                if table[next] == u8::MAX {
                    table[next] = depth + 1;
                    queue.push_back(next);
                }
            }
        }
        Self {
            spec,
            table,
            transitions,
        }
    }

    fn value(&self, idx: usize) -> u8 {
        self.table[idx]
    }

    fn next_index(&self, idx: usize, mv: Move) -> usize {
        let cached = self.transitions[idx][mv.idx()];
        debug_assert_ne!(cached, INVALID_MINI_TRANSITION);
        cached as usize
    }

    fn bytes(&self) -> usize {
        self.table.len() + self.transitions.len() * MOVE_COUNT * std::mem::size_of::<u32>()
    }
}

#[derive(Clone, Copy, Debug)]
struct MiniSpec {
    kind: MiniKind,
}

impl MiniSpec {
    const fn corner() -> Self {
        Self {
            kind: MiniKind::Corner,
        }
    }

    const fn edge_choice(choice: u8) -> Self {
        Self {
            kind: MiniKind::EdgeChoice(choice),
        }
    }

    const fn edge3() -> Self {
        Self {
            kind: MiniKind::Edge3,
        }
    }

    const fn uf3() -> Self {
        Self {
            kind: MiniKind::Uf3,
        }
    }

    fn size(self) -> usize {
        match self.kind {
            MiniKind::Corner => crate::coord::CORNER_COUNT,
            MiniKind::EdgeChoice(_) => crate::coord::EDGE_CHOICE_COUNT,
            MiniKind::Edge3 => crate::coord::EDGE3_COUNT,
            MiniKind::Uf3 => crate::coord::CENTER3_COUNT,
        }
    }

    fn solved_index(self) -> usize {
        self.index_of_state(SearchState::from_coord(FtoCoord::solved()))
    }

    fn index_of_state(self, state: SearchState) -> usize {
        match self.kind {
            MiniKind::Corner => usize::from(state.corner),
            MiniKind::EdgeChoice(0) => usize::from(state.edge.e0),
            MiniKind::EdgeChoice(1) => usize::from(state.edge.e1),
            MiniKind::EdgeChoice(2) => usize::from(state.edge.e2),
            MiniKind::EdgeChoice(_) => usize::from(state.edge.e3),
            MiniKind::Edge3 => usize::from(state.edge3),
            MiniKind::Uf3 => usize::from(state.uf3),
        }
    }

    fn next_index(self, idx: usize, mv: Move, tables: &TransitionTables) -> usize {
        match self.kind {
            MiniKind::Corner => usize::from(tables.corner_move(idx as u16, mv)),
            MiniKind::EdgeChoice(_) => usize::from(tables.edge_choice_move(idx as u16, mv)),
            MiniKind::Edge3 => usize::from(tables.edge3_move(idx as u16, mv)),
            MiniKind::Uf3 => usize::from(tables.uf_center3_move(idx as u16, mv)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum MiniKind {
    Corner,
    EdgeChoice(u8),
    Edge3,
    Uf3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BidirectionalChoice {
    Use {
        estimated_ida_nodes: u128,
        estimated_bidir_nodes: u128,
        estimated_stored_paths: u128,
    },
    Skip {
        estimated_ida_nodes: u128,
        estimated_bidir_nodes: u128,
        estimated_stored_paths: u128,
    },
}

#[must_use]
pub fn should_use_bidirectional_exact(
    depth: u8,
    allowed_moves: &[Move],
    max_stored_paths: usize,
    endpoint_count: usize,
) -> BidirectionalChoice {
    let commute = move_commutation();
    let first = allowed_moves.len() as u128;
    let avg_follow = if allowed_moves.is_empty() {
        0.0
    } else {
        let total: usize = allowed_moves
            .iter()
            .map(|&last| {
                allowed_moves
                    .iter()
                    .filter(|&&mv| !should_skip_after(&commute, last, mv))
                    .count()
            })
            .sum();
        total as f64 / allowed_moves.len() as f64
    };
    let tree = |target_depth: u8| -> TreeEstimate {
        if target_depth == 0 {
            return TreeEstimate { nodes: 1, leaves: 1 };
        }
        let mut nodes = 1_u128;
        let mut layer = first.max(1);
        for ply in 1..=target_depth {
            nodes = nodes.saturating_add(layer);
            if ply != target_depth {
                layer = ((layer as f64) * avg_follow).ceil() as u128;
            }
        }
        TreeEstimate { nodes, leaves: layer }
    };
    let fwd_depth = depth / 2;
    let back_depth = depth - fwd_depth;
    let estimated_ida_nodes = tree(depth).nodes;
    let endpoint_count = endpoint_count.max(1) as u128;
    let forward = tree(fwd_depth);
    let backward = tree(back_depth);
    choose_bidirectional(
        estimated_ida_nodes,
        forward.nodes.saturating_add(endpoint_count.saturating_mul(backward.nodes)),
        endpoint_count.saturating_mul(backward.leaves),
        max_stored_paths,
    )
}

#[must_use]
pub fn should_use_bidirectional_exact_with_pruning(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    depth: u8,
    allowed_moves: &[Move],
    max_stored_paths: usize,
    endpoint_count: usize,
) -> BidirectionalChoice {
    let start = SearchState::from_coord(coord);
    let solved = SearchState::from_coord(FtoCoord::solved());
    let fwd_depth = depth / 2;
    let back_depth = depth - fwd_depth;
    let endpoint_count = endpoint_count.max(1) as u128;

    let ida = estimate_pruned_tree(start, tables, pruning, depth, 0, allowed_moves);
    let forward = estimate_pruned_tree(start, tables, pruning, fwd_depth, back_depth, allowed_moves);
    let backward = estimate_pruned_tree(solved, tables, None, back_depth, fwd_depth, allowed_moves);

    choose_bidirectional(
        ida.nodes,
        forward.nodes.saturating_add(endpoint_count.saturating_mul(backward.nodes)),
        endpoint_count.saturating_mul(backward.leaves),
        max_stored_paths,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TreeEstimate {
    nodes: u128,
    leaves: u128,
}

fn choose_bidirectional(
    estimated_ida_nodes: u128,
    estimated_bidir_nodes: u128,
    estimated_stored_paths: u128,
    max_stored_paths: usize,
) -> BidirectionalChoice {
    let wins = estimated_bidir_nodes < estimated_ida_nodes
        && estimated_stored_paths <= max_stored_paths as u128;
    if wins {
        BidirectionalChoice::Use {
            estimated_ida_nodes,
            estimated_bidir_nodes,
            estimated_stored_paths,
        }
    } else {
        BidirectionalChoice::Skip {
            estimated_ida_nodes,
            estimated_bidir_nodes,
            estimated_stored_paths,
        }
    }
}

fn estimate_pruned_tree(
    start: SearchState,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    depth: u8,
    goal_slack: u8,
    allowed_moves: &[Move],
) -> TreeEstimate {
    const PROBE_DEPTH: u8 = 8;
    const MAX_PROBE_STATES: usize = 200_000;

    let commute = move_commutation();
    let root_bound = goal_slack.saturating_add(depth);
    if pruning_value_for_state(start, pruning) > root_bound {
        return TreeEstimate { nodes: 1, leaves: 0 };
    }
    if depth == 0 {
        return TreeEstimate { nodes: 1, leaves: 1 };
    }

    let mut nodes = 1_u128;
    let mut current = vec![(start, None, depth)];
    let mut current_count = 1_u128;
    let mut observed_branch = 0.0_f64;
    let mut remaining_depth = depth;
    let probe_depth = depth.min(PROBE_DEPTH);

    for _ in 0..probe_depth {
        let mut next = Vec::new();
        let mut next_count = 0_u128;
        for &(state, last_move, depth_left) in &current {
            let child_depth = depth_left - 1;
            for &mv in allowed_moves {
                if last_move.is_some_and(|last| should_skip_after(&commute, last, mv)) {
                    continue;
                }
                let pruning_child = state.apply_pruning(tables, mv);
                if pruning_value_for_child(pruning_child, pruning) > goal_slack + child_depth {
                    continue;
                }
                next_count += 1;
                if next.len() < MAX_PROBE_STATES {
                    next.push((
                        state.apply_with_pruning_child(tables, mv, pruning_child),
                        Some(mv),
                        child_depth,
                    ));
                }
            }
        }
        remaining_depth = remaining_depth.saturating_sub(1);
        nodes = nodes.saturating_add(next_count);
        if next_count == 0 {
            return TreeEstimate { nodes, leaves: 0 };
        }
        observed_branch = next_count as f64 / current_count.max(1) as f64;
        current_count = next_count;
        if next.len() as u128 != next_count {
            break;
        }
        current = next;
        if current.first().is_some_and(|&(_, _, depth_left)| depth_left == 0) {
            return TreeEstimate {
                nodes,
                leaves: current_count,
            };
        }
    }

    if remaining_depth == 0 {
        return TreeEstimate {
            nodes,
            leaves: current_count,
        };
    }
    let mut layer = current_count;
    for _ in 0..remaining_depth {
        layer = scaled_ceil(layer, observed_branch);
        nodes = nodes.saturating_add(layer);
        if layer == 0 {
            break;
        }
    }
    TreeEstimate { nodes, leaves: layer }
}

fn scaled_ceil(value: u128, factor: f64) -> u128 {
    if value == 0 || factor <= 0.0 {
        return 0;
    }
    let scaled = (value as f64 * factor).ceil();
    if scaled >= u128::MAX as f64 {
        u128::MAX
    } else {
        scaled as u128
    }
}

fn pruning_value_for_state(state: SearchState, pruning: Option<&SolverPruning>) -> u8 {
    pruning
        .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
        .unwrap_or(0)
}

fn pruning_value_for_child(child: PruningChild, pruning: Option<&SolverPruning>) -> u8 {
    pruning
        .map(|pruning| pruning.heuristic(child.edge3_uf3_idx, child.corner_uf3_idx))
        .unwrap_or(0)
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
    solve_with_pruning_threads_impl(coord, tables, pruning, None, config, threads, None)
}

#[must_use]
pub fn solve_with_pruning_and_mini_threads(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    mini: Option<&MiniPruning>,
    config: &SearchConfig,
    threads: usize,
) -> SearchResult {
    solve_with_pruning_threads_impl(coord, tables, pruning, mini, config, threads, None)
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
    solve_with_pruning_threads_impl(coord, tables, pruning, None, config, threads, Some(&report))
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
        solution_reporter: config.solution_reporter.clone(),
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
            for (prefix, start_cubie, start_state, _) in
                last_layer_start_states(cubie, state, tables, &moves)
            {
                ctx.nodes += 1;
                if let Some(suffix) = last_layer_free_u_suffix(start_cubie, &moves) {
                    push_unique_solution(
                        &mut ctx.solutions,
                        free_auf_solution(prefix, &[], suffix),
                    );
                    if let Some(solution) = ctx.solutions.last() {
                        report_solution(config, solution);
                    }
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
                if is_u_turn(mv) {
                    continue;
                }
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
                        ctx.free_prefix = prefix;
                        ctx.path.push(mv);
                        ctx.dfs(next_cubie, next_state, child_depth, Some(mv), true);
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
                let result = handle.join().expect("last-layer search worker panicked");
                depth_result.nodes += result.nodes;
                depth_result.solutions.extend(result.solutions);
            }
        });

        total.nodes += depth_result.nodes;
        total.solutions.extend(depth_result.solutions);
        dedup_solutions(&mut total.solutions);
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
            ctx.free_prefix = prefix;
            ctx.dfs(start_cubie, start_state, depth, last_move, false);
            ctx.free_prefix = None;
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
    mini: Option<&MiniPruning>,
    config: &SearchConfig,
    threads: usize,
    report: Option<&DepthReporter<'_>>,
) -> SearchResult {
    if threads <= 1 || config.max_depth <= 1 {
        return solve_with_pruning_single(coord, tables, pruning, mini, config, report);
    }

    let root = SearchState::from_coord(coord).with_mini_indices(mini);
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
                    report_solution(config, total.solutions.last().expect("solution just pushed"));
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
                let state =
                    start.apply_with_pruning_child_and_mini(tables, mv, pruning_child, mini);
                let mini_pruning_value = mini
                    .map(|mini| {
                        adjusted_pruning_value(
                            mini.heuristic(&state.mini_indices),
                            config.free_u_ends,
                        )
                    })
                    .unwrap_or(0);
                if mini_pruning_value > child_depth {
                    continue;
                }
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
                        mini,
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

pub fn solve_last_layer_bidirectional(
    cubie: FtoCubie,
    config: &BidirectionalConfig,
) -> Result<SearchResult, String> {
    let fwd_depth = config.depth / 2;
    let back_depth = config.depth - fwd_depth;
    let moves = move_cubies();
    let commute = move_commutation();
    let allowed_moves = config.allowed_moves.as_slice();
    let endpoints = last_layer_goal_cubies();
    let mut back = CubieBackwardBuilder {
        moves,
        commute,
        allowed_moves,
        max_stored_paths: config.max_stored_paths,
        stored_paths: 0,
        nodes: 0,
        paths: HashMap::new(),
    };
    for endpoint in endpoints {
        back.build(endpoint, back_depth, None, 0)?;
    }

    let mut matcher = CubieForwardMatcher {
        moves,
        commute,
        allowed_moves,
        back_paths: &back.paths,
        find_all: config.find_all,
        solutions: Vec::new(),
        nodes: back.nodes,
        path: Vec::with_capacity(config.depth as usize),
        reject_u_ends: true,
    };
    for (prefix, start, last_move) in [
        (None, cubie, None),
        (
            Some(Move::U),
            cubie.compose(&moves[Move::U.idx()]),
            Some(Move::U),
        ),
        (
            Some(Move::Up),
            cubie.compose(&moves[Move::Up.idx()]),
            Some(Move::Up),
        ),
    ] {
        if let Some(prefix) = prefix {
            matcher.path.push(prefix);
        }
        matcher.search(start, fwd_depth, last_move);
        if prefix.is_some() {
            matcher.path.pop();
        }
        if !config.find_all && !matcher.solutions.is_empty() {
            break;
        }
    }
    dedup_solutions(&mut matcher.solutions);
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
    mini: Option<&MiniPruning>,
    config: &SearchConfig,
    report: Option<&DepthReporter<'_>>,
) -> SearchResult {
    let mut ctx = SearchContext {
        tables,
        pruning,
        mini,
        commute: move_commutation(),
        config,
        solved: SearchState::from_coord(FtoCoord::solved()),
        solved_u: SearchState::from_coord(FtoCubie::solved().apply(Move::U).coord()),
        solved_up: SearchState::from_coord(FtoCubie::solved().apply(Move::Up).coord()),
        solutions: Vec::new(),
        path: Vec::with_capacity(config.max_depth as usize),
        nodes: 0,
    };
    let state = SearchState::from_coord(coord).with_mini_indices(mini);

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
    mini: Option<&'a MiniPruning>,
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
    free_prefix: Option<Move>,
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
            free_prefix: None,
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
                push_unique_solution(
                    &mut self.solutions,
                    free_auf_solution(self.free_prefix, &self.path, suffix),
                );
                if let Some(solution) = self.solutions.last() {
                    report_solution(self.config, solution);
                }
            }
            return;
        }

        let child_depth = depth_left - 1;
        let mut children = [(0_u8, Move::U, cubie, state); MOVE_COUNT];
        let mut child_count = 0;
        for &mv in &self.config.allowed_moves {
            if self.config.free_u_ends && self.path.is_empty() && is_u_turn(mv) {
                continue;
            }
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
                report_solution(self.config, self.solutions.last().expect("solution just pushed"));
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
            let next =
                state.apply_with_pruning_child_and_mini(self.tables, mv, pruning_child, self.mini);
            let mini_pruning_value = self
                .mini
                .map(|mini| {
                    adjusted_pruning_value(
                        mini.heuristic(&next.mini_indices),
                        self.config.free_u_ends,
                    )
                })
                .unwrap_or(0);
            if mini_pruning_value > child_depth {
                continue;
            }
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
        let base = adjusted_pruning_value(
            self.pruning
                .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
                .unwrap_or(0),
            self.config.free_u_ends,
        );
        let mini = self
            .mini
            .map(|mini| adjusted_pruning_value(mini.heuristic(&state.mini_indices), self.config.free_u_ends))
            .unwrap_or(0);
        base.max(mini)
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

fn is_u_turn(mv: Move) -> bool {
    matches!(mv, Move::U | Move::Up)
}

fn adjusted_pruning_value(value: u8, free_u_ends: bool) -> u8 {
    if value == u8::MAX {
        return value;
    }
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

fn push_unique_solution(solutions: &mut Vec<Vec<Move>>, solution: Vec<Move>) {
    if !solutions.contains(&solution) {
        solutions.push(solution);
    }
}

fn report_solution(config: &SearchConfig, solution: &[Move]) {
    if let Some(report) = config.solution_reporter.as_ref() {
        report(solution);
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

struct CubieBackwardBuilder<'a> {
    moves: [FtoCubie; MOVE_COUNT],
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    allowed_moves: &'a [Move],
    max_stored_paths: usize,
    stored_paths: usize,
    nodes: u64,
    paths: HashMap<FtoCubie, Vec<u64>>,
}

impl CubieBackwardBuilder<'_> {
    fn build(
        &mut self,
        state: FtoCubie,
        depth_left: u8,
        last_move: Option<Move>,
        encoded_path: u64,
    ) -> Result<(), String> {
        self.nodes += 1;
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
            let next = state.compose(&self.moves[mv.idx()]);
            self.build(
                next,
                depth_left - 1,
                Some(mv),
                encoded_path * MOVE_COUNT as u64 + mv.idx() as u64 + 1,
            )?;
        }
        Ok(())
    }
}

struct CubieForwardMatcher<'a> {
    moves: [FtoCubie; MOVE_COUNT],
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    allowed_moves: &'a [Move],
    back_paths: &'a HashMap<FtoCubie, Vec<u64>>,
    find_all: bool,
    solutions: Vec<Vec<Move>>,
    nodes: u64,
    path: Vec<Move>,
    reject_u_ends: bool,
}

impl CubieForwardMatcher<'_> {
    fn search(&mut self, state: FtoCubie, depth_left: u8, last_move: Option<Move>) {
        self.nodes += 1;
        if depth_left == 0 {
            if let Some(back_paths) = self.back_paths.get(&state) {
                for &encoded_back in back_paths {
                    let suffix = decode_inverse_path(encoded_back);
                    if self.reject_u_ends
                        && (ends_in_u_or_up(&self.path)
                            || matches!(suffix.last(), Some(&Move::U) | Some(&Move::Up)))
                    {
                        continue;
                    }
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
            if self.reject_u_ends && self.path.is_empty() && is_u_turn(mv) {
                continue;
            }
            if last_move.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            self.path.push(mv);
            self.search(state.compose(&self.moves[mv.idx()]), depth_left - 1, Some(mv));
            self.path.pop();
            if !self.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }
}

fn last_layer_goal_cubies() -> Vec<FtoCubie> {
    let mut goals = Vec::new();
    let solved = FtoCubie::solved();
    let mut uf = solved.uf;
    let mut rl = solved.rl;
    permute_center_colors(&mut uf, 0, &mut |uf| {
        permute_last_layer_rl(&mut rl, 0, &mut |rl| {
            let mut goal = solved;
            goal.uf = *uf;
            goal.rl = *rl;
            goals.push(goal);
        });
    });
    goals
}

fn permute_center_colors(centers: &mut [u8; 12], color: u8, emit: &mut impl FnMut(&[u8; 12])) {
    if color == 4 {
        emit(centers);
        return;
    }
    let start = color as usize * 3;
    permute_slice(centers, start, start + 3, &mut |centers| {
        permute_center_colors(centers, color + 1, emit);
    });
}

fn permute_last_layer_rl(centers: &mut [u8; 12], color: u8, emit: &mut impl FnMut(&[u8; 12])) {
    if color == 4 {
        emit(centers);
        return;
    }
    const FIXED: [Option<u8>; 4] = [None, Some(3), Some(8), Some(10)];
    let start = color as usize * 3;
    if let Some(fixed) = FIXED[color as usize] {
        let mut slots = Vec::new();
        for pos in start..start + 3 {
            if pos != fixed as usize {
                slots.push(pos);
            }
        }
        let a = slots[0];
        let b = slots[1];
        permute_last_layer_rl(centers, color + 1, emit);
        centers.swap(a, b);
        permute_last_layer_rl(centers, color + 1, emit);
        centers.swap(a, b);
    } else {
        permute_slice(centers, start, start + 3, &mut |centers| {
            permute_last_layer_rl(centers, color + 1, emit);
        });
    }
}

fn permute_slice(
    values: &mut [u8; 12],
    start: usize,
    end: usize,
    emit: &mut impl FnMut(&mut [u8; 12]),
) {
    fn rec(
        values: &mut [u8; 12],
        end: usize,
        idx: usize,
        emit: &mut impl FnMut(&mut [u8; 12]),
    ) {
        if idx == end {
            emit(values);
            return;
        }
        for swap in idx..end {
            values.swap(idx, swap);
            rec(values, end, idx + 1, emit);
            values.swap(idx, swap);
        }
    }
    rec(values, end, start, emit);
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

#[derive(Clone, Copy, Debug)]
struct SearchState {
    corner: u16,
    edge: EdgeCoord,
    edge3: u16,
    uf_center: u32,
    uf3: u16,
    rl_center: u32,
    edge3_uf3_idx: usize,
    corner_uf3_idx: usize,
    mini_indices: [usize; MAX_MINI_TABLES],
}

impl PartialEq for SearchState {
    fn eq(&self, other: &Self) -> bool {
        self.corner == other.corner
            && self.edge == other.edge
            && self.edge3 == other.edge3
            && self.uf_center == other.uf_center
            && self.uf3 == other.uf3
            && self.rl_center == other.rl_center
    }
}

impl Eq for SearchState {}

impl Hash for SearchState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.corner.hash(state);
        self.edge.hash(state);
        self.edge3.hash(state);
        self.uf_center.hash(state);
        self.uf3.hash(state);
        self.rl_center.hash(state);
    }
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
            mini_indices: [0; MAX_MINI_TABLES],
        }
    }

    fn with_mini_indices(mut self, mini: Option<&MiniPruning>) -> Self {
        if let Some(mini) = mini {
            self.mini_indices = mini.indices_of_state(self);
        }
        self
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
            mini_indices: self.mini_indices,
        }
    }

    fn apply_with_pruning_child_and_mini(
        self,
        tables: &TransitionTables,
        mv: Move,
        pruning_child: PruningChild,
        mini: Option<&MiniPruning>,
    ) -> Self {
        let mut next = self.apply_with_pruning_child(tables, mv, pruning_child);
        if let Some(mini) = mini {
            next.mini_indices = mini.move_indices(&self.mini_indices, mv);
        }
        next
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
                solution_reporter: None,
            },
        );

        assert_eq!(format_solution(&result.solutions[0]), "U' R'");
    }
}
