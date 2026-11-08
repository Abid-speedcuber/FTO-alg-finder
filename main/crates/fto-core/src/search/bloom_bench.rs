//! Manual experiments, excluded from normal builds.
use super::*;
use std::{collections::HashSet, path::Path, time::Instant};

// All equality fields, without search-only indices. Exact deduplication; no lossy hashes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct StateKey {
    corner: u16,
    edge0: u16,
    edge1: u16,
    edge3: u16,
    uf_center: u32,
    uf3: u16,
    rl_center: u32,
}
impl From<SearchState> for StateKey {
    fn from(s: SearchState) -> Self {
        Self {
            corner: s.corner,
            edge0: s.edge0,
            edge1: s.edge1,
            edge3: s.edge3,
            uf_center: s.uf_center,
            uf3: s.uf3,
            rl_center: s.rl_center,
        }
    }
}
impl StateKey {
    fn step(self, tables: &TransitionTables, mv: Move) -> Self {
        Self {
            corner: tables.corner_move(self.corner, mv),
            edge0: tables.edge_choice_move(self.edge0, mv),
            edge1: tables.edge_choice_move(self.edge1, mv),
            edge3: tables.edge3_move(self.edge3, mv),
            uf_center: tables.uf_center_move(self.uf_center, mv),
            uf3: tables.uf_center3_move(self.uf3, mv),
            rl_center: tables.rl_center_move(self.rl_center, mv),
        }
    }
}
pub(super) struct BloomTable {
    blocked: bool,
    block_offset: usize,
    pub(super) depth: u8,
    bits: Vec<u64>,
}
impl BloomTable {
    fn hashes(key: StateKey) -> (u64, u64) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        let a = h.finish();
        let mut b = a.wrapping_add(0x9e3779b97f4a7c15);
        b = (b ^ (b >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        b = (b ^ (b >> 27)).wrapping_mul(0x94d049bb133111eb);
        (a, (b ^ (b >> 31)) | 1)
    }
    fn positions(&self, key: StateKey) -> [usize; 7] {
        let (a, b) = Self::hashes(key);
        if self.blocked {
            let block = self.block_offset + (a as usize & (self.bits.len() / 8 - 1)) * 8;
            return std::array::from_fn(|i| (block + i) * 64 + ((b >> (i * 9)) & 63) as usize);
        }
        let mask = (self.bits.len() * 64 - 1) as u64;
        std::array::from_fn(|i| (a.wrapping_add((i as u64).wrapping_mul(b)) & mask) as usize)
    }
    fn contains_key(&self, key: StateKey) -> bool {
        self.positions(key)
            .iter()
            .all(|&bit| self.bits[bit / 64] & (1 << (bit % 64)) != 0)
    }
    pub(super) fn contains(&self, state: SearchState) -> bool {
        self.contains_key(state.into())
    }
}
fn memory_kib() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .filter(|line| line.starts_with("VmRSS:") || line.starts_with("VmHWM:"))
        .map(|s| {
            s.replace(':', "_KiB=")
                .replace("kB", "")
                .split_whitespace()
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn build_filters(
    tables: &TransitionTables,
    moves: &[Move],
    depths: &[u8],
    max_bytes: usize,
) -> Vec<BloomTable> {
    assert!(moves.iter().all(|mv| moves.contains(&mv.inverse())));
    let start = Instant::now();
    let solved = StateKey::from(SearchState::from_coord(FtoCoord::solved()));
    let mut seen = HashSet::from([solved]);
    let mut frontier = vec![solved];
    let mut filters = Vec::new();
    let max_depth = *depths.iter().max().unwrap();
    let count_only = std::env::var_os("BLOOM_COUNTS_ONLY").is_some();
    let max_states: usize = std::env::var("BLOOM_MAX_STATES")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(usize::MAX);
    let commute = move_commutation();
    let mut words = vec![0_u128; moves.len()];
    let mut words_total = 1_u128;
    let mut edge_attempts = 0_usize;
    // BFS uses an exact set; Bloom false positives never suppress construction states.
    for depth in 0..=max_depth {
        let bits = (seen.len() * 10).next_power_of_two().max(64);
        println!(
            "LAYER depth={} frontier={} cumulative={} filter_bytes={} elapsed_ms={:.3} {}",
            depth,
            frontier.len(),
            seen.len(),
            bits / 8,
            start.elapsed().as_secs_f64() * 1000.,
            memory_kib()
        );
        let words_layer = if depth == 0 { 1 } else { words.iter().sum() };
        println!(
            "CURVE depth={} new_states={} states={} words_layer={} words_total={} edges={}",
            depth,
            frontier.len(),
            seen.len(),
            words_layer,
            words_total,
            edge_attempts
        );
        if !count_only && depths.contains(&depth) {
            assert!(
                bits / 8 <= max_bytes,
                "complete layer exceeds Bloom memory budget"
            );
            let filter_start = Instant::now();
            let mut filter = BloomTable {
                blocked: false,
                block_offset: 0,
                depth,
                bits: vec![0; bits / 64],
            };
            for &key in &seen {
                for bit in filter.positions(key) {
                    filter.bits[bit / 64] |= 1 << (bit % 64);
                }
            }
            assert!(seen.iter().all(|&key| filter.contains_key(key)));
            println!(
                "FILTER depth={} states={} bytes={} populate_verify_ms={:.3} build_elapsed_ms={:.3} {}",
                depth,
                seen.len(),
                filter.bits.len() * 8,
                filter_start.elapsed().as_secs_f64() * 1000.,
                start.elapsed().as_secs_f64() * 1000.,
                memory_kib()
            );
            filters.push(filter);
        }
        if depth == max_depth {
            break;
        }
        let mut next_words = vec![0_u128; moves.len()];
        if depth == 0 {
            next_words.fill(1);
        } else {
            for (i, &last) in moves.iter().enumerate() {
                for (j, &current) in moves.iter().enumerate() {
                    if !should_skip_after(&commute, last, current) {
                        next_words[j] += words[i];
                    }
                }
            }
        }
        words = next_words;
        words_total += words.iter().sum::<u128>();
        edge_attempts = frontier.len() * moves.len();
        let mut next = Vec::new();
        for key in frontier {
            for &mv in moves {
                let child = key.step(tables, mv);
                if seen.insert(child) {
                    next.push(child);
                    if seen.len() > max_states {
                        println!(
                            "TRUNCATED depth={} partial_states={} limit={}",
                            depth + 1,
                            seen.len(),
                            max_states
                        );
                        assert!(
                            std::env::var_os("BLOOM_BUILD_ONLY").is_some(),
                            "state budget exhausted during solve benchmark"
                        );
                        return filters;
                    }
                }
            }
        }
        frontier = next;
    }
    filters
}
fn run(
    start: FtoCoord,
    tables: &TransitionTables,
    pruning: &SolverPruning,
    config: &SearchConfig,
    bloom: Option<&BloomTable>,
) -> SearchResult {
    assert!(!config.free_u_ends);
    let mut ctx = SearchContext {
        tail_bloom: bloom_for_config(config),
        tables,
        pruning: Some(pruning),
        move_pruning: pruning.for_moves(),
        mini: None,
        bloom,
        commute: move_commutation(),
        config,
        solved: SearchState::from_coord(FtoCoord::solved()),
        solved_u: SearchState::from_coord(FtoCubie::solved().apply(Move::U).coord()),
        solved_up: SearchState::from_coord(FtoCubie::solved().apply(Move::Up).coord()),
        solutions: Vec::new(),
        path: Vec::new(),
        nodes: 0,
    };
    let state = SearchState::from_coord(start).with_pruning_indices(Some(pruning), true, start);
    for depth in config.min_depth..=config.max_depth {
        ctx.dfs(state, depth, None, None);
        if !ctx.solutions.is_empty() {
            break;
        }
    }
    SearchResult {
        solutions: ctx.solutions,
        nodes: ctx.nodes,
    }
}
#[test]
fn bloom_key_preserves_search_hash_and_identity() {
    let mut cubie = FtoCubie::solved();
    for i in 0..512 {
        cubie = cubie.apply(Move::ALL[i % MOVE_COUNT]);
        let state = SearchState::from_coord(cubie.coord());
        let mut full = std::collections::hash_map::DefaultHasher::new();
        state.hash(&mut full);
        assert_eq!(full.finish(), BloomTable::hashes(state.into()).0);
    }
}
#[test]
#[ignore = "loads real transition tables to validate compact BFS keys"]
fn bloom_key_transitions_match_cubies() {
    let tables = TransitionTables::load_or_build("../../cache/transition-table.tran").unwrap();
    let mut cubie = FtoCubie::solved();
    for i in 0..256 {
        let key = StateKey::from(SearchState::from_coord(cubie.coord()));
        for mv in Move::ALL {
            let expected = StateKey::from(SearchState::from_coord(cubie.apply(mv).coord()));
            assert_eq!(key.step(&tables, mv), expected);
        }
        cubie = cubie.apply(Move::ALL[i % MOVE_COUNT]);
    }
}
#[test]
#[ignore = "manual release-mode benchmark"]
fn depth_five_bloom_benchmark() {
    let tables = TransitionTables::load_or_build("../../cache/transition-table.tran").unwrap();
    let mut moves = vec![
        Move::U,
        Move::Up,
        Move::F,
        Move::Fp,
        Move::R,
        Move::Rp,
        Move::BL,
        Move::BLp,
    ];
    if let Ok(names) = std::env::var("BLOOM_MOVES") {
        moves.clear();
        for name in names.split(',') {
            let mv = Move::ALL
                .into_iter()
                .find(|mv| format!("{mv:?}") == name)
                .expect("unknown move");
            for item in [mv, mv.inverse()] {
                if !moves.contains(&item) {
                    moves.push(item);
                }
            }
        }
        assert!(!moves.is_empty());
    }
    println!("MOVESET {:?}", moves);
    let depths: Vec<u8> = std::env::var("BLOOM_DEPTHS")
        .unwrap_or_else(|_| "5".into())
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert!(depths.iter().all(|&d| d > 0));
    let max_mib: usize = std::env::var("BLOOM_MAX_MIB")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(200);
    println!(
        "SETUP depths={:?} max_filter_mib={} key_bytes={} {}",
        depths,
        max_mib,
        std::mem::size_of::<StateKey>(),
        memory_kib()
    );
    let filters = build_filters(&tables, &moves, &depths, max_mib * 1024 * 1024);
    if std::env::var_os("BLOOM_BUILD_ONLY").is_some() {
        return;
    }
    let metadata =
        crate::pruning::read_table_metadata("../../cache/pruning-v4/e3cf3_dcef.prun").unwrap();
    let suffix = metadata
        .lines()
        .find_map(|line| line.strip_prefix("moves="))
        .unwrap();
    let pruning_moves = crate::pruning::parse_move_set_suffix(suffix).unwrap();
    assert!(moves.iter().all(|mv| pruning_moves.contains(mv)));
    let pruning = SolverPruning::load_or_build_with_moves(
        &tables,
        Path::new("../../cache/pruning-v4"),
        0,
        &pruning_moves,
    )
    .unwrap();
    let mut cases = vec![
        vec![
            Move::R,
            Move::U,
            Move::Fp,
            Move::BL,
            Move::Up,
            Move::Rp,
            Move::F,
            Move::BLp,
            Move::U,
            Move::R,
            Move::Fp,
            Move::Up,
            Move::BL,
            Move::F,
            Move::Rp,
            Move::U,
        ],
        vec![
            Move::F,
            Move::Rp,
            Move::BL,
            Move::U,
            Move::Fp,
            Move::BLp,
            Move::Up,
            Move::R,
            Move::F,
            Move::U,
            Move::BL,
            Move::Rp,
            Move::Up,
            Move::Fp,
            Move::R,
            Move::BLp,
            Move::U,
        ],
    ];
    if let Ok(count) = std::env::var("BLOOM_CASES") {
        let count: usize = count.parse().expect("BLOOM_CASES must be an integer");
        assert!(count > 0);
        let mut rng = 0xd5b1000520261106_u64;
        let commute = move_commutation();
        cases.clear();
        for case in 0..count {
            let mut scramble = Vec::new();
            while scramble.len() < 16 + case % 2 {
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
            cases.push(scramble);
        }
        println!("SUITE cases={} seed=d5b1000520261106", count);
    }

    let cpu_hz: Option<f64> = std::env::var("BLOOM_CPU_HZ")
        .ok()
        .map(|s| s.parse().unwrap());
    let cpu_ticks = || -> u64 {
        let stat = std::fs::read_to_string("/proc/thread-self/stat").unwrap();
        let fields: Vec<_> = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .collect();
        fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
    };
    let repeats: usize = std::env::var("BLOOM_REPEATS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(3);
    for (case, scramble) in cases.iter().enumerate() {
        let cubie = scramble
            .iter()
            .fold(FtoCubie::solved(), |s, &mv| s.apply(mv));
        let depth = scramble.len() as u8;
        let config = SearchConfig {
            min_depth: depth,
            max_depth: depth,
            find_all: false,
            allowed_moves: moves.clone(),
            ..SearchConfig::default()
        };
        println!("CASE {} depth={} scramble={:?}", case + 1, depth, scramble);
        let mut expected = None;
        for round in 0..=repeats {
            // Rotate and reverse mode order across rounds/cases, including baseline.
            let mut order: Vec<_> = (0..=filters.len()).collect();
            let len = order.len();
            order.rotate_left((case + round) % len);
            if round % 2 == 1 {
                order.reverse();
            }
            for mode in order {
                let filter = mode.checked_sub(1).map(|i| &filters[i]);
                let filter_depth = filter.map_or(0, |f| f.depth);
                let cpu_start = cpu_hz.map(|_| cpu_ticks());
                let start = Instant::now();
                let result = run(cubie.coord(), &tables, &pruning, &config, filter);
                let ms = start.elapsed().as_secs_f64() * 1000.;
                let cpu_ms = cpu_hz
                    .zip(cpu_start)
                    .map_or(-1., |(hz, ticks)| (cpu_ticks() - ticks) as f64 * 1000. / hz);
                assert!(!result.solutions.is_empty());
                let solution = &result.solutions[0];
                assert_eq!(solution.len(), depth as usize);
                assert_eq!(
                    solution.iter().fold(cubie, |s, &mv| s.apply(mv)).coord(),
                    FtoCoord::solved()
                );
                if let Some(ref previous) = expected {
                    assert_eq!(&result.solutions, previous);
                } else {
                    expected = Some(result.solutions.clone());
                }
                println!(
                    "RESULT case={} round={} bloom_depth={} ms={:.3} cpu_ms={:.3} nodes={} solution={:?}",
                    case + 1,
                    round,
                    filter_depth,
                    ms,
                    cpu_ms,
                    result.nodes,
                    solution
                );
            }
        }
    }
}

fn hll_count(registers: &[u8]) -> f64 {
    let m = registers.len() as f64;
    let raw = (0.7213 / (1.0 + 1.079 / m)) * m * m
        / registers
            .iter()
            .map(|&r| 2_f64.powi(-i32::from(r)))
            .sum::<f64>();
    let empty = registers.iter().filter(|&&r| r == 0).count();
    if raw <= 2.5 * m && empty > 0 {
        m * (m / empty as f64).ln()
    } else {
        raw
    }
}

struct StreamingCounts<'a> {
    tables: &'a TransitionTables,
    successors: Vec<Vec<Move>>,
    registers: Vec<Vec<u8>>,
    paths: Vec<u64>,
    nodes: u64,
    expected_nodes: u128,
    start: Instant,
}
impl StreamingCounts<'_> {
    fn visit(&mut self, key: StateKey, ply: usize, remaining: usize, last: usize) {
        const P: u32 = 18;
        let hash = BloomTable::hashes(key).0;
        let index = (hash >> (64 - P)) as usize;
        let rank = ((hash << P).leading_zeros() + 1).min(65 - P) as u8;
        let slot = &mut self.registers[ply][index];
        *slot = (*slot).max(rank);
        self.paths[ply] += 1;
        self.nodes += 1;
        if self.nodes & ((1 << 25) - 1) == 0 {
            let seconds = self.start.elapsed().as_secs_f64();
            let rate = self.nodes as f64 / seconds;
            println!(
                "STREAM_PROGRESS nodes={} expected={} elapsed_s={:.3} nodes_per_s={:.0} eta_s={:.1}",
                self.nodes,
                self.expected_nodes,
                seconds,
                rate,
                (self.expected_nodes as f64 - self.nodes as f64) / rate
            );
        }
        if remaining == 0 {
            return;
        }
        for i in 0..self.successors[last].len() {
            let mv = self.successors[last][i];
            self.visit(key.step(self.tables, mv), ply + 1, remaining - 1, mv.idx());
        }
    }
}

#[test]
#[ignore = "enumerates canonical sequences with memory-bounded cardinality sketches"]
fn streaming_growth_benchmark() {
    let tables = TransitionTables::load_or_build("../../cache/transition-table.tran").unwrap();
    let names = std::env::var("STREAM_MOVES").unwrap_or_else(|_| "R,U,F".into());
    let depth: usize = std::env::var("STREAM_DEPTH")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(12);
    assert!((1..=24).contains(&depth));
    let mut moves = Vec::new();
    for name in names.split(',') {
        let mv = Move::ALL
            .into_iter()
            .find(|mv| format!("{mv:?}") == name)
            .expect("unknown move");
        for item in [mv, mv.inverse()] {
            if !moves.contains(&item) {
                moves.push(item);
            }
        }
    }
    let commute = move_commutation();
    let mut successors = vec![Vec::new(); MOVE_COUNT + 1];
    successors[MOVE_COUNT] = moves.clone();
    for &last in &moves {
        successors[last.idx()] = moves
            .iter()
            .copied()
            .filter(|&mv| !should_skip_after(&commute, last, mv))
            .collect();
    }
    let mut expected = vec![1_u128];
    let mut word_counts = vec![1_u128; moves.len()];
    expected.push(moves.len() as u128);
    for _ in 2..=depth {
        let mut next = vec![0_u128; moves.len()];
        for (i, &last) in moves.iter().enumerate() {
            for (j, &mv) in moves.iter().enumerate() {
                if !should_skip_after(&commute, last, mv) {
                    next[j] += word_counts[i];
                }
            }
        }
        word_counts = next;
        expected.push(word_counts.iter().sum());
    }
    println!(
        "STREAM_SETUP moves={} depth={} expected_paths={} precision_bits=18 standard_error_pct={:.4} sketch_bytes={} {}",
        names,
        depth,
        expected.iter().sum::<u128>(),
        100. * 1.04 / ((1_u64 << 18) as f64).sqrt(),
        (depth + 1) * (1 << 18),
        memory_kib()
    );
    let mut ctx = StreamingCounts {
        tables: &tables,
        successors,
        registers: vec![vec![0; 1 << 18]; depth + 1],
        paths: vec![0; depth + 1],
        nodes: 0,
        expected_nodes: expected.iter().sum(),
        start: Instant::now(),
    };
    ctx.visit(
        StateKey::from(SearchState::from_coord(FtoCoord::solved())),
        0,
        depth,
        MOVE_COUNT,
    );
    assert_eq!(u128::from(ctx.nodes), ctx.expected_nodes);
    let mut cumulative = vec![0_u8; 1 << 18];
    let mut total_words = 0_u128;
    let mut previous_count = 0_f64;
    for (d, (&paths, registers)) in ctx.paths.iter().zip(&ctx.registers).enumerate() {
        assert_eq!(u128::from(paths), expected[d]);
        total_words += u128::from(paths);
        for (a, &b) in cumulative.iter_mut().zip(registers) {
            *a = (*a).max(b);
        }
        let count = hll_count(&cumulative);
        println!(
            "STREAM_CURVE depth={} paths_layer={} paths_total={} states_estimate={:.3} new_states_estimate={:.3} exact_length_unique_estimate={:.3}",
            d,
            paths,
            total_words,
            count,
            count - previous_count,
            hll_count(registers)
        );
        previous_count = count;
    }
    println!(
        "STREAM_DONE nodes={} elapsed_s={:.3} {}",
        ctx.nodes,
        ctx.start.elapsed().as_secs_f64(),
        memory_kib()
    );
}

mod overnight;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LookupStats {
    pub(super) queries: u64,
    pub(super) rejected: u64,
    pub(super) exact_probes: u64,
    pub(super) hits: u64,
}

mod bidir;

mod bidir_five;
