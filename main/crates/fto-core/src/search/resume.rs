//! Versioned, portable DFS continuations for exact-depth batch searches.
//! Only active paths/cursors are persisted; coordinate and pruning state is
//! reconstructed along these paths, never by replaying completed subtrees.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeCheckpoint {
    pub version: u8,
    pub depth: u8,
    pub input: Vec<u8>,
    pub tasks: Vec<Cursor>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cursor {
    pub root: u8,
    pub base: u8,
    pub path: Vec<u8>,
    pub next: Vec<u8>,
    pub entered: Vec<bool>,
}
impl Cursor {
    fn root(root: usize) -> Self {
        Self {
            root: root as u8,
            base: 0,
            path: vec![],
            next: vec![0],
            entered: vec![false],
        }
    }
}
pub struct ResumeResult {
    pub result: SearchResult,
    pub checkpoint: Option<ResumeCheckpoint>,
}
pub(crate) enum Visit {
    Pruned,
    Branch,
    Leaf(Option<Vec<Move>>),
}
pub(crate) trait Kernel: Sync {
    type State: Clone + Send + Sync;
    fn roots(&self) -> Vec<Self::State>;
    fn visit(&self, state: &Self::State, remaining: u8, root: usize, path: &[Move]) -> Visit;
    fn child(
        &self,
        state: &Self::State,
        mv: Move,
        remaining: u8,
        last: Option<Move>,
        at_root: bool,
    ) -> Option<Self::State>;
    fn replay(&self, state: &Self::State, mv: Move) -> Self::State;
    fn root_last(&self, root: usize) -> Option<Move> {
        [None, Some(Move::U), Some(Move::Up)][root]
    }
}
struct Frame<S> {
    state: S,
    next: usize,
    entered: bool,
}
struct Walk<S> {
    root: usize,
    base: usize,
    path: Vec<Move>,
    frames: Vec<Frame<S>>,
}
impl<S: Clone> Walk<S> {
    fn restore<K: Kernel<State = S>>(cursor: &Cursor, roots: &[S], kernel: &K) -> Self {
        let path: Vec<_> = cursor.path.iter().map(|&m| Move::ALL[m as usize]).collect();
        let mut state = roots[cursor.root as usize].clone();
        let mut frames = Vec::new();
        for level in 0..=path.len() {
            if level >= cursor.base as usize {
                let i = level - cursor.base as usize;
                frames.push(Frame {
                    state: state.clone(),
                    next: cursor.next[i] as usize,
                    entered: cursor.entered[i],
                });
            }
            if let Some(&mv) = path.get(level) {
                state = kernel.replay(&state, mv);
            }
        }
        Self {
            root: cursor.root as usize,
            base: cursor.base as usize,
            path,
            frames,
        }
    }
    fn cursor(&self) -> Cursor {
        Cursor {
            root: self.root as u8,
            base: self.base as u8,
            path: self.path.iter().map(|m| m.idx() as u8).collect(),
            next: self.frames.iter().map(|f| f.next as u8).collect(),
            entered: self.frames.iter().map(|f| f.entered).collect(),
        }
    }
    fn pop(&mut self) {
        self.frames.pop();
        if self.path.len() > self.base {
            self.path.pop();
        }
    }
}

pub(crate) fn signature(cubie: FtoCubie, config: &SearchConfig, partial: bool) -> Vec<u8> {
    let mut bytes = vec![
        u8::from(partial),
        u8::from(config.free_u_ends),
        u8::from(config.find_all),
    ];
    for orbit in [
        &cubie.cp[..],
        &cubie.co[..],
        &cubie.ep[..],
        &cubie.uf[..],
        &cubie.rl[..],
    ] {
        bytes.extend_from_slice(orbit);
    }
    bytes.push(config.allowed_moves.len() as u8);
    bytes.extend(config.allowed_moves.iter().map(|m| m.idx() as u8));
    bytes
}
fn validate(
    cp: &ResumeCheckpoint,
    config: &SearchConfig,
    input: &[u8],
    roots: usize,
) -> Result<(), String> {
    if cp.version != 1 || cp.depth != config.max_depth || cp.input != input {
        return Err(
            "Search checkpoint does not match this case, depth, moves, or goal mode.".into(),
        );
    }
    if cp.tasks.len() > 65536 {
        return Err("Too many saved search stacks.".into());
    }
    for c in &cp.tasks {
        let base = c.base as usize;
        if c.root as usize >= roots
            || base > c.path.len()
            || c.path.len() > cp.depth as usize
            || c.next.len() != c.path.len() - base + 1
            || c.entered.len() != c.next.len()
            || c.path.iter().any(|&m| {
                m as usize >= MOVE_COUNT || !config.allowed_moves.contains(&Move::ALL[m as usize])
            })
            || c.next
                .iter()
                .any(|&n| n as usize > config.allowed_moves.len())
            || c.entered
                .iter()
                .zip(&c.next)
                .any(|(&entered, &n)| !entered && n != 0)
            || (c.path.len() == cp.depth as usize && *c.entered.last().unwrap_or(&false))
            || c.entered[..c.entered.len() - 1].iter().any(|&e| !e)
        {
            return Err("Invalid saved DFS stack.".into());
        }
        for i in 0..c.next.len() - 1 {
            let child = c.path[base + i];
            if c.next[i] == 0
                || config.allowed_moves[c.next[i] as usize - 1].idx() != child as usize
            {
                return Err("Invalid saved DFS branch cursor.".into());
            }
        }
    }
    Ok(())
}
fn emit(
    config: &SearchConfig,
    solution: Vec<Move>,
    found: &Mutex<Vec<Vec<Move>>>,
    seen: &Mutex<HashSet<Vec<Move>>>,
    stop: &AtomicBool,
) {
    let mut seen = seen.lock().unwrap();
    if (!config.find_all && stop.load(Ordering::Relaxed)) || !seen.insert(solution.clone()) {
        return;
    }
    if !config.find_all {
        stop.store(true, Ordering::Relaxed);
    }
    report_solution(config, &solution);
    found.lock().unwrap().push(solution);
}

// Split an active stack from its ancestor continuation, or expand one pending
// node. These are disjoint jobs, so resumed searches can change thread counts.
fn split<K: Kernel>(
    cursor: Cursor,
    roots: &[K::State],
    kernel: &K,
    config: &SearchConfig,
    nodes: &mut u64,
    found: &Mutex<Vec<Vec<Move>>>,
    seen: &Mutex<HashSet<Vec<Move>>>,
    stop: &AtomicBool,
) -> Vec<Cursor> {
    if cursor.next.len() > 1 {
        let mut continuation = cursor.clone();
        continuation.path.pop();
        continuation.next.pop();
        continuation.entered.pop();
        let active = Cursor {
            root: cursor.root,
            base: cursor.path.len() as u8,
            path: cursor.path,
            next: vec![*cursor.next.last().unwrap()],
            entered: vec![*cursor.entered.last().unwrap()],
        };
        return vec![continuation, active];
    }
    let walk = Walk::restore(&cursor, roots, kernel);
    let frame = &walk.frames[0];
    let remaining = config.max_depth - walk.path.len() as u8;
    if !frame.entered {
        *nodes += 1;
        match kernel.visit(&frame.state, remaining, walk.root, &walk.path) {
            Visit::Pruned => return vec![],
            Visit::Leaf(solution) => {
                if let Some(solution) = solution {
                    emit(config, solution, found, seen, stop);
                }
                return vec![];
            }
            Visit::Branch => {}
        }
    }
    let last = walk
        .path
        .last()
        .copied()
        .or_else(|| kernel.root_last(walk.root));
    let mut jobs = Vec::new();
    for &mv in &config.allowed_moves[frame.next..] {
        if kernel
            .child(&frame.state, mv, remaining - 1, last, walk.path.is_empty())
            .is_some()
        {
            let mut path = cursor.path.clone();
            path.push(mv.idx() as u8);
            jobs.push(Cursor {
                root: cursor.root,
                base: path.len() as u8,
                path,
                next: vec![0],
                entered: vec![false],
            });
        }
    }
    jobs
}

pub(crate) fn run<K: Kernel>(
    kernel: &K,
    config: &SearchConfig,
    threads: usize,
    input: Vec<u8>,
    checkpoint: Option<&ResumeCheckpoint>,
) -> Result<ResumeResult, String> {
    if config.min_depth != config.max_depth || config.allowed_moves.len() > MOVE_COUNT {
        return Err("Resumable searches require one exact depth and unique allowed moves.".into());
    }
    let threads = threads.clamp(1, 1024);
    let mut moves = HashSet::new();
    if config.allowed_moves.iter().any(|m| !moves.insert(*m)) {
        return Err("Allowed moves must be unique.".into());
    }
    let roots = kernel.roots();
    let mut jobs: VecDeque<_> = if let Some(cp) = checkpoint {
        validate(cp, config, &input, roots.len())?;
        cp.tasks.clone().into()
    } else {
        (0..roots.len()).map(Cursor::root).collect()
    };
    let found = Mutex::new(Vec::new());
    let seen = Mutex::new(HashSet::new());
    let stop = AtomicBool::new(false);
    let mut nodes = 0_u64;
    // Partition only until there are enough jobs, without materializing a tree.
    let target = threads.clamp(1, 1024).saturating_mul(4);
    while threads > 1
        && !jobs.is_empty()
        && jobs.len() < target
        && !is_cancelled(config)
        && !stop.load(Ordering::Relaxed)
    {
        let job = jobs.pop_front().unwrap();
        jobs.extend(split(
            job, &roots, kernel, config, &mut nodes, &found, &seen, &stop,
        ));
    }
    let queue = Mutex::new(jobs);
    let held = Mutex::new(Vec::new());
    let worker = || {
        let mut nodes = 0;
        loop {
            if is_cancelled(config) || stop.load(Ordering::Relaxed) {
                break;
            }
            let Some(cursor) = queue.lock().unwrap().pop_front() else {
                break;
            };
            let mut walk = Walk::restore(&cursor, &roots, kernel);
            while !walk.frames.is_empty() {
                if is_cancelled(config) || stop.load(Ordering::Relaxed) {
                    break;
                }
                let remaining = config.max_depth - walk.path.len() as u8;
                let frame = walk.frames.last_mut().unwrap();
                if !frame.entered {
                    nodes += 1;
                    match kernel.visit(&frame.state, remaining, walk.root, &walk.path) {
                        Visit::Pruned => {
                            walk.pop();
                            continue;
                        }
                        Visit::Leaf(solution) => {
                            if let Some(solution) = solution {
                                emit(config, solution, &found, &seen, &stop);
                            }
                            walk.pop();
                            continue;
                        }
                        Visit::Branch => frame.entered = true,
                    }
                }
                if frame.next == config.allowed_moves.len() {
                    walk.pop();
                    continue;
                }
                let mv = config.allowed_moves[frame.next];
                frame.next += 1;
                let last = walk
                    .path
                    .last()
                    .copied()
                    .or_else(|| kernel.root_last(walk.root));
                if let Some(state) =
                    kernel.child(&frame.state, mv, remaining - 1, last, walk.path.is_empty())
                {
                    walk.path.push(mv);
                    walk.frames.push(Frame {
                        state,
                        next: 0,
                        entered: false,
                    });
                }
            }
            if !walk.frames.is_empty() {
                held.lock().unwrap().push(walk.cursor());
            }
        }
        nodes
    };
    let worker_count = threads.min(queue.lock().unwrap().len());
    let worker_nodes = if worker_count <= 1 {
        worker()
    } else {
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..worker_count).map(|_| scope.spawn(&worker)).collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("resumable search worker panicked"))
                .sum::<u64>()
        })
    };
    nodes += worker_nodes;
    let solutions = found.into_inner().unwrap();
    let mut tasks: Vec<_> = queue.into_inner().unwrap().into();
    tasks.extend(held.into_inner().unwrap());
    let checkpoint = if tasks.is_empty() || (!config.find_all && !solutions.is_empty()) {
        None
    } else {
        Some(ResumeCheckpoint {
            version: 1,
            depth: config.max_depth,
            input,
            tasks,
        })
    };
    Ok(ResumeResult {
        result: SearchResult { solutions, nodes },
        checkpoint,
    })
}

struct ExactKernel<'a> {
    ctx: SearchContext<'a>,
    roots: Vec<SearchState>,
}
impl Kernel for ExactKernel<'_> {
    type State = (SearchState, Option<u8>);
    fn roots(&self) -> Vec<Self::State> {
        self.roots.iter().map(|&state| (state, None)).collect()
    }
    fn visit(
        &self,
        &(state, raw): &Self::State,
        remaining: u8,
        root: usize,
        path: &[Move],
    ) -> Visit {
        let ctx = &self.ctx;
        let raw = raw.or_else(|| ctx.raw_pruning_value(state));
        if raw
            .map(|v| adjusted_pruning_value(v, ctx.config.free_u_ends))
            .unwrap_or(0)
            > remaining
            || ctx
                .tail_bloom
                .is_some_and(|t| remaining == t.depth() && !t.contains(state.bloom_state()))
        {
            return Visit::Pruned;
        }
        if remaining > 0 {
            return Visit::Branch;
        }
        Visit::Leaf(
            free_u_suffix_with_raw_pruning(
                state,
                ctx.solved,
                ctx.solved_u,
                ctx.solved_up,
                ctx.config.free_u_ends,
                raw,
            )
            .map(|suffix| free_auf_solution(self.root_last(root), path, suffix)),
        )
    }
    fn child(
        &self,
        &(state, _): &Self::State,
        mv: Move,
        remaining: u8,
        last: Option<Move>,
        _at_root: bool,
    ) -> Option<Self::State> {
        let ctx = &self.ctx;
        if last.is_some_and(|last| ctx.should_skip_after(last, mv)) {
            return None;
        }
        let child = state.apply_pruning_bounded(
            ctx.tables,
            mv,
            ctx.move_pruning,
            remaining.saturating_add(u8::from(ctx.config.free_u_ends)),
        );
        let raw = ctx.raw_pruning_value_for_child(child);
        if raw
            .map(|v| adjusted_pruning_value(v, ctx.config.free_u_ends))
            .unwrap_or(0)
            > remaining
        {
            return None;
        }
        Some((state.apply_with_pruning_child(ctx.tables, mv, child), raw))
    }
    fn replay(&self, &(state, _): &Self::State, mv: Move) -> Self::State {
        let child = state.apply_pruning_with(self.ctx.tables, mv, self.ctx.move_pruning);
        (
            state.apply_with_pruning_child(self.ctx.tables, mv, child),
            self.ctx.raw_pruning_value_for_child(child),
        )
    }
}
struct LlKernel<'a> {
    ctx: LastLayerSearchContext<'a>,
    roots: Vec<LastLayerState>,
}
impl Kernel for LlKernel<'_> {
    type State = (LastLayerState, bool);
    fn roots(&self) -> Vec<Self::State> {
        self.roots.iter().map(|&s| (s, false)).collect()
    }
    fn visit(
        &self,
        &(state, checked): &Self::State,
        remaining: u8,
        root: usize,
        path: &[Move],
    ) -> Visit {
        let ctx = &self.ctx;
        if (!checked && ctx.pruning_value(state) > remaining)
            || ctx
                .tail_bloom
                .is_some_and(|t| remaining == t.depth() && !t.contains_hashes(state.bloom_hashes()))
        {
            return Visit::Pruned;
        }
        if remaining > 0 {
            return Visit::Branch;
        }
        if ends_in_u_or_up(path) {
            return Visit::Leaf(None);
        }
        Visit::Leaf(
            last_layer_free_u_suffix(state, ctx.goal, ctx.tables, &ctx.rl_center_pos_moves)
                .map(|suffix| free_auf_solution(self.root_last(root), path, suffix)),
        )
    }
    fn child(
        &self,
        &(state, _): &Self::State,
        mv: Move,
        remaining: u8,
        last: Option<Move>,
        at_root: bool,
    ) -> Option<Self::State> {
        let ctx = &self.ctx;
        if (at_root && is_u_turn(mv))
            || last.is_some_and(|last| should_skip_after(&ctx.commute, last, mv))
        {
            return None;
        }
        let child = state.apply_pruning_with(ctx.tables, mv, ctx.move_pruning);
        if ctx.pruning_value_for_child(child) > remaining {
            return None;
        }
        Some((
            state.apply_with_pruning_child(ctx.tables, mv, child, &ctx.rl_center_pos_moves),
            true,
        ))
    }
    fn replay(&self, &(state, _): &Self::State, mv: Move) -> Self::State {
        let ctx = &self.ctx;
        let child = state.apply_pruning_with(ctx.tables, mv, ctx.move_pruning);
        (
            state.apply_with_pruning_child(ctx.tables, mv, child, &ctx.rl_center_pos_moves),
            true,
        )
    }
}

pub fn resume_exact(
    cubie: FtoCubie,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    ll_pruning: Option<&PatternDatabases>,
    config: &SearchConfig,
    threads: usize,
    checkpoint: Option<&ResumeCheckpoint>,
) -> Result<ResumeResult, String> {
    let input = signature(cubie, config, false);
    if config.free_u_ends {
        let pruning = ll_pruning.map(LastLayerPruning::Patterns);
        let pos_moves = center_position_moves(CenterOrbitForPosition::Rl);
        let state = LastLayerState::from_cubie(cubie).with_pruning_indices(pruning);
        let roots = last_layer_start_states(state, tables, &pos_moves, pruning)
            .into_iter()
            .map(|(_, s, _)| s)
            .collect();
        let ctx = LastLayerSearchContext::new(
            tables,
            pruning,
            config,
            pos_moves,
            LastLayerGoal::solved(),
        );
        run(&LlKernel { ctx, roots }, config, threads, input, checkpoint)
    } else {
        let coord = cubie.coord();
        let state = SearchState::from_coord(coord).with_pruning_indices(pruning, true, coord);
        let ctx = SearchContext {
            tail_bloom: bloom_for_config(config),
            #[cfg(test)]
            bloom: None,
            tables,
            pruning,
            move_pruning: pruning.and_then(SolverPruning::for_moves),
            mini: None,
            commute: move_commutation(),
            config,
            solved: SearchState::from_coord(FtoCoord::solved()),
            solved_u: SearchState::from_coord(FtoCubie::solved().apply(Move::U).coord()),
            solved_up: SearchState::from_coord(FtoCubie::solved().apply(Move::Up).coord()),
            solutions: vec![],
            path: vec![],
            nodes: 0,
        };
        run(
            &ExactKernel {
                ctx,
                roots: vec![state],
            },
            config,
            threads,
            input,
            checkpoint,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Tree<'a> {
        visits: &'a Mutex<HashMap<Vec<Move>, usize>>,
        cancel: Arc<AtomicBool>,
        left: AtomicUsize,
    }
    impl Kernel for Tree<'_> {
        type State = Vec<Move>;
        fn roots(&self) -> Vec<Self::State> {
            vec![vec![]]
        }
        fn visit(&self, state: &Self::State, remaining: u8, _root: usize, _path: &[Move]) -> Visit {
            *self
                .visits
                .lock()
                .unwrap()
                .entry(state.clone())
                .or_default() += 1;
            if self.left.fetch_sub(1, Ordering::Relaxed) == 1 {
                self.cancel.store(true, Ordering::Relaxed);
            }
            if remaining == 0 {
                Visit::Leaf(Some(state.clone()))
            } else {
                Visit::Branch
            }
        }
        fn child(
            &self,
            state: &Self::State,
            mv: Move,
            _remaining: u8,
            _last: Option<Move>,
            _at_root: bool,
        ) -> Option<Self::State> {
            Some(self.replay(state, mv))
        }
        fn replay(&self, state: &Self::State, mv: Move) -> Self::State {
            let mut next = state.clone();
            next.push(mv);
            next
        }
    }
    #[test]
    fn exact_continuations_never_revisit_nodes_when_threads_change() {
        let cancel = Arc::new(AtomicBool::new(false));
        let config = SearchConfig {
            min_depth: 5,
            max_depth: 5,
            allowed_moves: vec![Move::R, Move::U, Move::F],
            cancel: Some(cancel.clone()),
            ..SearchConfig::default()
        };
        let visits = Mutex::new(HashMap::new());
        let mut cp = None;
        let mut nodes = 0;
        let mut solutions = HashSet::new();
        for turn in 0..1000 {
            cancel.store(false, Ordering::Relaxed);
            let kernel = Tree {
                visits: &visits,
                cancel: cancel.clone(),
                left: AtomicUsize::new(7),
            };
            let result = run(
                &kernel,
                &config,
                [1, 4, 2, 8][turn % 4],
                vec![7],
                cp.as_ref(),
            )
            .unwrap();
            nodes += result.result.nodes;
            for s in result.result.solutions {
                assert!(solutions.insert(s), "a solution was replayed");
            }
            cp = result.checkpoint.map(|checkpoint| {
                serde_json::from_slice(&serde_json::to_vec(&checkpoint).unwrap()).unwrap()
            });
            if cp.is_none() {
                break;
            }
        }
        assert!(cp.is_none());
        assert_eq!(solutions.len(), 3_usize.pow(5));
        assert_eq!(nodes, (0..=5).map(|i| 3_u64.pow(i)).sum::<u64>());
        assert_eq!(visits.lock().unwrap().len() as u64, nodes);
        assert!(visits.lock().unwrap().values().all(|&count| count == 1));
    }
    #[test]
    fn malformed_and_mismatched_checkpoints_are_rejected() {
        let cancel = Arc::new(AtomicBool::new(true));
        let visits = Mutex::new(HashMap::new());
        let config = SearchConfig {
            min_depth: 2,
            max_depth: 2,
            allowed_moves: vec![Move::R],
            cancel: Some(cancel.clone()),
            ..SearchConfig::default()
        };
        let kernel = Tree {
            visits: &visits,
            cancel,
            left: AtomicUsize::new(10),
        };
        let cp = run(&kernel, &config, 1, vec![1], None)
            .unwrap()
            .checkpoint
            .unwrap();
        let mut bad = cp.clone();
        bad.version = 2;
        assert!(run(&kernel, &config, 1, vec![1], Some(&bad)).is_err());
        assert!(run(&kernel, &config, 1, vec![2], Some(&cp)).is_err());
        let mut bad = cp.clone();
        bad.tasks[0].path = vec![255];
        assert!(run(&kernel, &config, 1, vec![1], Some(&bad)).is_err());
        let mut bad = cp.clone();
        bad.tasks[0].next.clear();
        assert!(run(&kernel, &config, 1, vec![1], Some(&bad)).is_err());
    }
    #[test]
    #[ignore = "integration check uses the locally generated transition table"]
    fn real_full_ll_and_partial_searches_resume_from_json_without_replaying_nodes() {
        use crate::partial::{PartialMask, PartialProblem, PreparedPartialSearch};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let tables = TransitionTables::load(root.join("cache/transition-table.tran")).unwrap();
        let cubie = FtoCubie::solved()
            .apply(Move::R)
            .apply(Move::U)
            .apply(Move::R);
        for (free, depth) in [(false, 3), (true, 1)] {
            let start = if free {
                FtoCubie::solved()
                    .apply(Move::U)
                    .apply(Move::R)
                    .apply(Move::U)
            } else {
                cubie
            };
            let config = SearchConfig {
                min_depth: depth,
                max_depth: depth,
                free_u_ends: free,
                allowed_moves: vec![Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp],
                ..SearchConfig::default()
            };
            let legacy = if free {
                solve_last_layer_with_pattern_pruning_threads(start, &tables, None, &config, 1)
            } else {
                solve_with_pruning_and_mini_threads(start.coord(), &tables, None, None, &config, 1)
            };
            let expected = resume_exact(start, &tables, None, None, &config, 1, None).unwrap();
            assert!(!expected.result.solutions.is_empty());
            assert_eq!(
                legacy.solutions.iter().cloned().collect::<HashSet<_>>(),
                expected.result.solutions.iter().cloned().collect()
            );
            let cancel = Arc::new(AtomicBool::new(false));
            let trigger = cancel.clone();
            let paused_config = SearchConfig {
                cancel: Some(cancel.clone()),
                solution_reporter: Some(Arc::new(move |_| trigger.store(true, Ordering::Relaxed))),
                ..config.clone()
            };
            let first = resume_exact(start, &tables, None, None, &paused_config, 1, None).unwrap();
            let cp: ResumeCheckpoint =
                serde_json::from_slice(&serde_json::to_vec(&first.checkpoint.unwrap()).unwrap())
                    .unwrap();
            cancel.store(false, Ordering::Relaxed);
            let rest = resume_exact(start, &tables, None, None, &config, 4, Some(&cp)).unwrap();
            assert!(rest.checkpoint.is_none());
            assert_eq!(
                first.result.nodes + rest.result.nodes,
                expected.result.nodes,
                "nodes were replayed in mode free={free}"
            );
            let solutions: HashSet<_> = first
                .result
                .solutions
                .into_iter()
                .chain(rest.result.solutions)
                .collect();
            assert_eq!(solutions, expected.result.solutions.into_iter().collect());
            let wrong = SearchConfig {
                allowed_moves: vec![Move::R],
                ..config.clone()
            };
            assert!(resume_exact(start, &tables, None, None, &wrong, 1, Some(&cp)).is_err());
        }
        let mut mask = PartialMask::all();
        mask.corners.fill(false);
        mask.edges.fill(false);
        mask.uf_centers.fill(false);
        mask.rl_centers.fill(false);
        // A deliberately partial goal makes many leaves solutions, exercising pause-after-report.
        mask.edges[0] = true;
        let problem = PartialProblem { cubie, mask };
        let config = SearchConfig {
            min_depth: 3,
            max_depth: 3,
            allowed_moves: vec![Move::R, Move::Rp, Move::U, Move::Up],
            ..SearchConfig::default()
        };
        let prepared = PreparedPartialSearch::new(&problem, &config, None);
        let expected = prepared.resume_depth(&config, 1, None).unwrap();
        let legacy = prepared.solve_depth(&config, 3, 1);
        assert!(!expected.result.solutions.is_empty());
        assert_eq!(
            legacy.solutions.into_iter().collect::<HashSet<_>>(),
            expected.result.solutions.iter().cloned().collect()
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let paused = SearchConfig {
            cancel: Some(cancel),
            solution_reporter: Some(Arc::new(move |_| trigger.store(true, Ordering::Relaxed))),
            ..config.clone()
        };
        let first = prepared.resume_depth(&paused, 1, None).unwrap();
        let cp: ResumeCheckpoint =
            serde_json::from_slice(&serde_json::to_vec(&first.checkpoint.unwrap()).unwrap())
                .unwrap();
        // Rebuild pruning just as reopening the app does, then use a new thread budget.
        let reopened = PreparedPartialSearch::new(&problem, &config, None);
        let rest = reopened.resume_depth(&config, 4, Some(&cp)).unwrap();
        assert!(rest.checkpoint.is_none());
        assert_eq!(
            first.result.nodes + rest.result.nodes,
            expected.result.nodes
        );
        assert_eq!(
            first
                .result
                .solutions
                .into_iter()
                .chain(rest.result.solutions)
                .collect::<HashSet<_>>(),
            expected.result.solutions.into_iter().collect()
        );
    }
}
