//! Five-generator exact-depth test with a memory-bounded asymmetric split.
use super::*;

const NONE: u32 = u32::MAX;
#[derive(Clone, Copy)]
struct Ends {
    head: u32,
    tail: u32,
}
#[derive(Clone, Copy)]
struct Link {
    path: u64,
    next: u32,
}
struct BackMap {
    heads: HashMap<StateKey, Ends>,
    links: Vec<Link>,
    nodes: u64,
}
impl BackMap {
    fn build(
        &mut self,
        key: StateKey,
        left: u8,
        last: Option<Move>,
        path: u64,
        tables: &TransitionTables,
        moves: &[Move],
        commute: &[[bool; MOVE_COUNT]; MOVE_COUNT],
    ) {
        self.nodes += 1;
        if left == 0 {
            let index = u32::try_from(self.links.len()).unwrap();
            self.links.push(Link { path, next: NONE });
            self.heads
                .entry(key)
                .and_modify(|end| {
                    self.links[end.tail as usize].next = index;
                    end.tail = index;
                })
                .or_insert(Ends {
                    head: index,
                    tail: index,
                });
            return;
        }
        for &mv in moves {
            if last.is_some_and(|last| should_skip_after(commute, last, mv)) {
                continue;
            }
            self.build(
                key.step(tables, mv),
                left - 1,
                Some(mv),
                path * MOVE_COUNT as u64 + mv.idx() as u64 + 1,
                tables,
                moves,
                commute,
            );
        }
    }
    fn paths(&self, key: StateKey) -> Option<Paths<'_>> {
        self.heads.get(&key).map(|e| Paths {
            links: &self.links,
            next: e.head,
        })
    }
}
struct Paths<'a> {
    links: &'a [Link],
    next: u32,
}
impl Iterator for Paths<'_> {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        if self.next == NONE {
            return None;
        }
        let row = self.links[self.next as usize];
        self.next = row.next;
        Some(row.path)
    }
}
fn cpu_ms() -> f64 {
    let stat = std::fs::read_to_string("/proc/thread-self/stat").unwrap();
    let f: Vec<_> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    (f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()) as f64 * 10.
}
fn path_counts(
    moves: &[Move],
    commute: &[[bool; MOVE_COUNT]; MOVE_COUNT],
    depth: usize,
) -> Vec<usize> {
    let mut layers = vec![1, moves.len()];
    let mut counts = vec![1_usize; moves.len()];
    for _ in 2..=depth {
        let mut next = vec![0; moves.len()];
        for (i, &last) in moves.iter().enumerate() {
            for (j, &mv) in moves.iter().enumerate() {
                if !should_skip_after(commute, last, mv) {
                    next[j] += counts[i];
                }
            }
        }
        counts = next;
        layers.push(counts.iter().sum());
    }
    layers
}
fn filter_for(map: &BackMap, blocked: bool) -> BloomTable {
    let bits = (map.heads.len() * 10).next_power_of_two().max(512);
    let mut f = BloomTable {
        blocked,
        block_offset: 0,
        depth: 8,
        bits: vec![0; bits / 64 + if blocked { 7 } else { 0 }],
    };
    if blocked {
        f.block_offset = (64 - f.bits.as_ptr() as usize % 64) % 64 / 8;
        assert_eq!((f.bits.as_ptr() as usize + f.block_offset * 8) % 64, 0);
    }
    for &key in map.heads.keys() {
        for p in f.positions(key) {
            f.bits[p / 64] |= 1 << (p % 64);
        }
    }
    f
}
struct Matcher<'a> {
    tables: &'a TransitionTables,
    pruning: &'a SolverPruning,
    moves: &'a [Move],
    back: &'a BackMap,
    filter: Option<&'a BloomTable>,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    back_depth: u8,
    path: Vec<Move>,
    solution: Option<Vec<Move>>,
    stats: LookupStats,
    nodes: u64,
    capture: Vec<StateKey>,
    capture_limit: usize,
    started: Instant,
    max_seconds: f64,
    aborted: bool,
    label: String,
}
impl Matcher<'_> {
    fn visit(&mut self, state: SearchState, left: u8, last: Option<Move>) {
        if self.aborted {
            return;
        }
        self.nodes += 1;
        if self.nodes & ((1 << 20) - 1) == 0 {
            let elapsed = self.started.elapsed().as_secs_f64();
            if self.nodes & ((1 << 24) - 1) == 0 {
                println!(
                    "FIVE_PROGRESS label={} nodes={} queries={} elapsed_s={:.2}",
                    self.label, self.nodes, self.stats.queries, elapsed
                );
            }
            if elapsed > self.max_seconds {
                self.aborted = true;
                return;
            }
        }
        if pruning_value_for_state(state, Some(self.pruning)) > self.back_depth + left {
            return;
        }
        if left == 0 {
            let key = StateKey::from(state);
            self.stats.queries += 1;
            if self.capture.len() < self.capture_limit {
                self.capture.push(key);
            }
            if self.filter.is_some_and(|f| !f.contains_key(key)) {
                self.stats.rejected += 1;
                return;
            }
            self.stats.exact_probes += 1;
            if let Some(paths) = self.back.paths(key) {
                self.stats.hits += 1;
                for encoded in paths {
                    let suffix = decode_inverse_path(encoded);
                    if let (Some(&last), Some(&first)) = (self.path.last(), suffix.first()) {
                        if should_skip_after(&self.commute, last, first) {
                            continue;
                        }
                    }
                    let mut solution = self.path.clone();
                    solution.extend(suffix);
                    self.solution = Some(solution);
                    return;
                }
            }
            return;
        }
        for &mv in self.moves {
            if last.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            let child = state.apply_pruning_bounded(
                self.tables,
                mv,
                self.pruning.for_moves(),
                self.back_depth + left - 1,
            );
            if pruning_value_for_child(child, Some(self.pruning)) > self.back_depth + left - 1 {
                continue;
            }
            self.path.push(mv);
            self.visit(
                state.apply_with_pruning_child(self.tables, mv, child),
                left - 1,
                Some(mv),
            );
            self.path.pop();
            if self.solution.is_some() || self.aborted {
                return;
            }
        }
    }
}

#[test]
#[ignore = "five-generator exact-depth bidirectional matching benchmark"]
fn five_generator_bidir_bloom_benchmark() {
    let tables = TransitionTables::load_or_build("../../cache/transition-table.tran").unwrap();
    let metadata =
        crate::pruning::read_table_metadata("../../cache/pruning-v4/e3cf3_dcef.prun").unwrap();
    let pruning_moves = crate::pruning::parse_move_set_suffix(
        metadata
            .lines()
            .find_map(|s| s.strip_prefix("moves="))
            .unwrap(),
    )
    .unwrap();
    let moves = vec![
        Move::R,
        Move::Rp,
        Move::U,
        Move::Up,
        Move::F,
        Move::Fp,
        Move::B,
        Move::Bp,
        Move::BL,
        Move::BLp,
    ];
    assert!(moves.iter().all(|m| pruning_moves.contains(m)));
    let commute = move_commutation();
    let counts = path_counts(&moves, &commute, 10);
    let back_depth: u8 = std::env::var("FIVE_BACK_DEPTH")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(8);
    let total_depth: u8 = std::env::var("FIVE_DEPTH")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(19);
    assert!((4..=10).contains(&back_depth) && total_depth > back_depth);
    let paths = counts[back_depth as usize];
    let buckets = (paths * 8).div_ceil(7).next_power_of_two();
    let projected = buckets * (std::mem::size_of::<(StateKey, Ends)>() + 1)
        + paths * std::mem::size_of::<Link>();
    let cap: usize = std::env::var("FIVE_MAP_MAX_MIB")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(1200)
        * 1024
        * 1024;
    println!(
        "FIVE_PLAN moves=R,U,F,B,BL depth={} forward={} backward={} backward_paths={} projected_map_bytes={} balanced_back10_paths={}",
        total_depth,
        total_depth - back_depth,
        back_depth,
        paths,
        projected,
        counts[10]
    );
    assert!(
        projected <= cap,
        "exact map exceeds benchmark memory budget"
    );
    // Prove the compact representation preserves every path and its ordering.
    let mut small = BackMap {
        heads: HashMap::new(),
        links: Vec::new(),
        nodes: 0,
    };
    small.build(
        StateKey::from(SearchState::from_coord(FtoCoord::solved())),
        4,
        None,
        0,
        &tables,
        &moves,
        &commute,
    );
    let mut reference = BackwardBuilder {
        tables: &tables,
        pruning: None,
        move_pruning: None,
        commute,
        allowed_moves: &moves,
        goal_slack: total_depth - 4,
        max_stored_paths: 10000,
        stored_paths: 0,
        nodes: 0,
        paths: HashMap::new(),
    };
    reference
        .build(SearchState::from_coord(FtoCoord::solved()), 4, None, 0)
        .unwrap();
    assert_eq!(small.nodes, reference.nodes);
    assert_eq!(small.heads.len(), reference.paths.len());
    assert_eq!(small.links.len(), reference.stored_paths);
    for (state, paths) in &reference.paths {
        assert_eq!(
            small.paths((*state).into()).unwrap().collect::<Vec<_>>(),
            *paths
        );
    }
    drop(small);
    drop(reference);
    let start = Instant::now();
    let cpu = cpu_ms();
    let mut back = BackMap {
        heads: HashMap::with_capacity(paths),
        links: Vec::with_capacity(paths),
        nodes: 0,
    };
    back.build(
        StateKey::from(SearchState::from_coord(FtoCoord::solved())),
        back_depth,
        None,
        0,
        &tables,
        &moves,
        &commute,
    );
    assert_eq!(back.links.len(), paths);
    println!(
        "FIVE_BACKWARD ms={:.3} cpu_ms={:.3} states={} paths={} nodes={} map_capacity={} key_bytes={} link_bytes={} {}",
        start.elapsed().as_secs_f64() * 1000.,
        cpu_ms() - cpu,
        back.heads.len(),
        back.links.len(),
        back.nodes,
        back.heads.capacity(),
        std::mem::size_of::<StateKey>(),
        std::mem::size_of::<Link>(),
        memory_kib()
    );
    let mut filters = Vec::new();
    for blocked in [false, true] {
        let start = Instant::now();
        let cpu = cpu_ms();
        let f = filter_for(&back, blocked);
        println!(
            "FIVE_FILTER layout={} ms={:.3} cpu_ms={:.3} bytes={}",
            if blocked { "blocked" } else { "standard" },
            start.elapsed().as_secs_f64() * 1000.,
            cpu_ms() - cpu,
            f.bits.len() * 8
        );
        assert!(back.heads.keys().all(|&key| f.contains_key(key)));
        filters.push(f);
    }
    // Load pruning only after backward construction to avoid construction peaks overlapping cache IO buffers.
    let pruning = SolverPruning::load_or_build_with_moves(
        &tables,
        Path::new("../../cache/pruning-v4"),
        0,
        &pruning_moves,
    )
    .unwrap();
    let cases: usize = std::env::var("FIVE_CASES")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(3);
    let repeats: usize = std::env::var("FIVE_REPEATS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(2);
    let max_seconds: f64 = std::env::var("FIVE_SEARCH_SECONDS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(180.);
    let mut rng = 0xb1d1900261107_u64;
    let mut query_sample = Vec::new();
    for case in 1..=cases {
        let mut scramble = Vec::new();
        while scramble.len() < usize::from(total_depth) {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let mv = moves[(rng as usize) % moves.len()];
            if scramble
                .last()
                .is_some_and(|&last| should_skip_after(&commute, last, mv))
            {
                continue;
            }
            scramble.push(mv);
        }
        let cubie = scramble
            .iter()
            .fold(FtoCubie::solved(), |s, &mv| s.apply(mv));
        let coord = cubie.coord();
        println!("FIVE_CASE case={} scramble={:?}", case, scramble);
        let mut expected = None;
        let mut nodes = None;
        let mut hits = None;
        for round in 0..=repeats {
            let mut order = vec![0_usize, 1, 2];
            if round > 0 {
                order.rotate_left((case + round) % 3);
                if round % 2 == 1 {
                    order.reverse();
                }
            }
            for mode in order {
                let layout = ["baseline", "standard", "blocked"][mode];
                let mut matcher = Matcher {
                    tables: &tables,
                    pruning: &pruning,
                    moves: &moves,
                    back: &back,
                    filter: mode.checked_sub(1).map(|i| &filters[i]),
                    commute,
                    back_depth,
                    path: Vec::with_capacity(total_depth as usize),
                    solution: None,
                    stats: LookupStats::default(),
                    nodes: 0,
                    capture: Vec::new(),
                    capture_limit: if case == 1 && round == 0 && mode == 0 {
                        1_000_000
                    } else {
                        0
                    },
                    started: Instant::now(),
                    max_seconds,
                    aborted: false,
                    label: format!("c{case}-r{round}-{layout}"),
                };
                let start = Instant::now();
                let cpu = cpu_ms();
                matcher.visit(
                    SearchState::from_coord(coord).with_pruning_indices(
                        Some(&pruning),
                        true,
                        coord,
                    ),
                    total_depth - back_depth,
                    None,
                );
                let ms = start.elapsed().as_secs_f64() * 1000.;
                let cpu = cpu_ms() - cpu;
                println!(
                    "FIVE_MATCH case={} round={} layout={} ms={:.3} cpu_ms={:.3} nodes={} queries={} rejected={} exact_probes={} hits={} solved={} aborted={}",
                    case,
                    round,
                    layout,
                    ms,
                    cpu,
                    matcher.nodes,
                    matcher.stats.queries,
                    matcher.stats.rejected,
                    matcher.stats.exact_probes,
                    matcher.stats.hits,
                    matcher.solution.is_some(),
                    matcher.aborted
                );
                assert!(
                    !matcher.aborted,
                    "per-search time limit exceeded; result is incomplete"
                );
                let solution = matcher.solution.unwrap();
                assert_eq!(solution.len(), total_depth as usize);
                assert_eq!(
                    solution.iter().fold(cubie, |s, &mv| s.apply(mv)).coord(),
                    FtoCoord::solved()
                );
                if let Some(ref old) = expected {
                    assert_eq!(*old, solution);
                } else {
                    expected = Some(solution);
                }
                if let Some(old) = nodes {
                    assert_eq!(old, matcher.nodes);
                } else {
                    nodes = Some(matcher.nodes);
                }
                if let Some(old) = hits {
                    assert_eq!(old, matcher.stats.hits);
                } else {
                    hits = Some(matcher.stats.hits);
                }
                if !matcher.capture.is_empty() {
                    query_sample = matcher.capture;
                }
            }
        }
    }
    for round in 0..=repeats {
        let mut order = vec![0_usize, 1, 2];
        order.rotate_left(round % 3);
        if round % 2 == 1 {
            order.reverse();
        }
        let mut expected = None;
        for mode in order {
            let filter = mode.checked_sub(1).map(|i| &filters[i]);
            let mut stats = LookupStats::default();
            let start = Instant::now();
            let cpu = cpu_ms();
            for _ in 0..3 {
                for &key in &query_sample {
                    stats.queries += 1;
                    let key = std::hint::black_box(key);
                    if filter.is_some_and(|f| !f.contains_key(key)) {
                        stats.rejected += 1;
                        continue;
                    }
                    stats.exact_probes += 1;
                    if std::hint::black_box(back.heads.get(&key)).is_some() {
                        stats.hits += 1;
                    }
                }
            }
            let ms = start.elapsed().as_secs_f64() * 1000.;
            let cpu = cpu_ms() - cpu;
            if let Some(old) = expected {
                assert_eq!(old, stats.hits);
            } else {
                expected = Some(stats.hits);
            }
            println!(
                "FIVE_LOOKUP round={} layout={} ms={:.3} cpu_ms={:.3} queries={} rejected={} exact_probes={} hits={}",
                round,
                ["baseline", "standard", "blocked"][mode],
                ms,
                cpu,
                stats.queries,
                stats.rejected,
                stats.exact_probes,
                stats.hits
            );
        }
    }
    println!("FIVE_DONE {}", memory_kib());
}
