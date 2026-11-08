//! Compare exact bidirectional matching with and without a Bloom prefilter.
use super::*;

fn cpu_ms() -> f64 {
    let stat = std::fs::read_to_string("/proc/thread-self/stat").unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    (fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()) as f64 * 10.
}
fn filter_for(map: &HashMap<SearchState, Vec<u64>>, blocked: bool) -> BloomTable {
    let bits = (map.len() * 10).next_power_of_two().max(64);
    let mut filter = BloomTable {
        blocked,
        block_offset: 0,
        depth: 10,
        bits: vec![0; bits / 64 + if blocked { 7 } else { 0 }],
    };
    if blocked {
        filter.block_offset = (64 - filter.bits.as_ptr() as usize % 64) % 64 / 8;
        assert_eq!(
            (filter.bits.as_ptr() as usize + filter.block_offset * 8) % 64,
            0
        );
    }
    for &state in map.keys() {
        for bit in filter.positions(state.into()) {
            filter.bits[bit / 64] |= 1 << (bit % 64);
        }
    }
    filter
}
fn match_one(
    start: FtoCoord,
    tables: &TransitionTables,
    pruning: &SolverPruning,
    moves: &[Move],
    map: &HashMap<SearchState, Vec<u64>>,
    filter: Option<&BloomTable>,
) -> (SearchResult, LookupStats) {
    let mut matcher = ForwardMatcher {
        bloom: filter,
        lookup_stats: LookupStats::default(),
        tables,
        pruning: Some(pruning),
        move_pruning: pruning.for_moves(),
        commute: move_commutation(),
        allowed_moves: moves,
        goal_slack: 10,
        back_paths: map,
        find_all: false,
        solutions: Vec::new(),
        nodes: 0,
        path: Vec::with_capacity(19),
    };
    matcher.search(
        SearchState::from_coord(start).with_pruning_indices(Some(pruning), true, start),
        9,
        None,
    );
    (
        SearchResult {
            solutions: matcher.solutions,
            nodes: matcher.nodes,
        },
        matcher.lookup_stats,
    )
}
struct QueryCollector<'a> {
    tables: &'a TransitionTables,
    pruning: &'a SolverPruning,
    moves: &'a [Move],
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    queries: Vec<SearchState>,
}
impl QueryCollector<'_> {
    fn visit(&mut self, state: SearchState, depth: u8, last: Option<Move>) {
        if pruning_value_for_state(state, Some(self.pruning)) > 10 + depth {
            return;
        }
        if depth == 0 {
            self.queries.push(state);
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
                10 + depth - 1,
            );
            if pruning_value_for_child(child, Some(self.pruning)) > 10 + depth - 1 {
                continue;
            }
            self.visit(
                state.apply_with_pruning_child(self.tables, mv, child),
                depth - 1,
                Some(mv),
            );
        }
    }
}
fn probes(
    queries: &[SearchState],
    map: &HashMap<SearchState, Vec<u64>>,
    filter: Option<&BloomTable>,
    passes: usize,
) -> LookupStats {
    let mut stats = LookupStats::default();
    for _ in 0..passes {
        for &state in queries {
            let state = std::hint::black_box(state);
            stats.queries += 1;
            if filter.is_some_and(|f| !f.contains(state)) {
                stats.rejected += 1;
                continue;
            }
            stats.exact_probes += 1;
            if std::hint::black_box(map.get(&state)).is_some() {
                stats.hits += 1;
            }
        }
    }
    std::hint::black_box(stats)
}

#[test]
#[ignore = "manual depth-19 bidirectional/Bloom matching benchmark"]
fn depth_nineteen_bidir_bloom_benchmark() {
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
    let pruning = SolverPruning::load_or_build_with_moves(
        &tables,
        Path::new("../../cache/pruning-v4"),
        0,
        &pruning_moves,
    )
    .unwrap();
    let case_count: usize = std::env::var("BIDIR_CASES")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(6);
    let repeats: usize = std::env::var("BIDIR_REPEATS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(3);
    let sets = std::env::var("BIDIR_SETS").unwrap_or_else(|_| "R,U,F;U,F,B".into());
    for (set_index, names) in sets.split(';').enumerate() {
        let mut moves = Vec::new();
        for name in names.split(',') {
            let mv = Move::ALL
                .into_iter()
                .find(|m| format!("{m:?}") == name)
                .unwrap();
            for item in [mv, mv.inverse()] {
                if !moves.contains(&item) {
                    moves.push(item);
                }
            }
        }
        assert!(moves.iter().all(|mv| pruning_moves.contains(mv)));
        let commute = move_commutation();
        let mut back = BackwardBuilder {
            tables: &tables,
            pruning: None,
            move_pruning: None,
            commute,
            allowed_moves: &moves,
            goal_slack: 9,
            max_stored_paths: 2_000_000,
            stored_paths: 0,
            nodes: 0,
            paths: HashMap::new(),
        };
        let start = Instant::now();
        let cpu = cpu_ms();
        back.build(SearchState::from_coord(FtoCoord::solved()), 10, None, 0)
            .unwrap();
        let build_ms = start.elapsed().as_secs_f64() * 1000.;
        let build_cpu = cpu_ms() - cpu;
        println!(
            "BACKWARD set={} build_ms={:.3} cpu_ms={:.3} states={} paths={} nodes={} state_key_bytes={} {}",
            names,
            build_ms,
            build_cpu,
            back.paths.len(),
            back.stored_paths,
            back.nodes,
            std::mem::size_of::<SearchState>(),
            memory_kib()
        );
        let mut filters = Vec::new();
        for blocked in [false, true] {
            let start = Instant::now();
            let cpu = cpu_ms();
            let filter = filter_for(&back.paths, blocked);
            println!(
                "PREFILTER set={} layout={} build_ms={:.3} cpu_ms={:.3} bytes={}",
                names,
                if blocked { "blocked" } else { "standard" },
                start.elapsed().as_secs_f64() * 1000.,
                cpu_ms() - cpu,
                filter.bits.len() * 8
            );
            assert!(back.paths.keys().all(|&state| filter.contains(state)));
            filters.push(filter);
        }
        let mut rng = 0xb1d1900261107_u64;
        let mut micro_start = None;
        for case in 1..=case_count {
            let mut scramble = Vec::new();
            while scramble.len() < 19 {
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
            micro_start.get_or_insert(cubie.coord());
            println!(
                "BIDIR_CASE set={} case={} depth=19 scramble={:?}",
                names, case, scramble
            );
            let mut expected = None;
            let mut expected_nodes = None;
            let mut expected_hits = None;
            for round in 0..=repeats {
                let mut order = vec![0_usize, 1, 2];
                order.rotate_left((case + round + set_index) % 3);
                if round % 2 == 1 {
                    order.reverse();
                }
                for mode in order {
                    let layout = ["baseline", "standard", "blocked"][mode];
                    let filter = mode.checked_sub(1).map(|i| &filters[i]);
                    let timing_passes = if back.paths.len() < 500_000 { 10 } else { 2 };
                    let cpu = cpu_ms();
                    let start = Instant::now();
                    let mut last_result: Option<(SearchResult, LookupStats)> = None;
                    for _ in 0..timing_passes {
                        let result = match_one(
                            cubie.coord(),
                            &tables,
                            &pruning,
                            &moves,
                            &back.paths,
                            filter,
                        );
                        if let Some((ref previous, previous_stats)) = last_result {
                            assert_eq!(result.0.solutions, previous.solutions);
                            assert_eq!(result.1, previous_stats);
                        }
                        last_result = Some(result);
                    }
                    let ms = start.elapsed().as_secs_f64() * 1000. / timing_passes as f64;
                    let cpu = (cpu_ms() - cpu) / timing_passes as f64;
                    let (result, stats) = last_result.unwrap();
                    assert!(!result.solutions.is_empty());
                    let solution = &result.solutions[0];
                    assert_eq!(solution.len(), 19);
                    assert_eq!(
                        solution.iter().fold(cubie, |s, &mv| s.apply(mv)).coord(),
                        FtoCoord::solved()
                    );
                    if let Some(ref expected) = expected {
                        assert_eq!(&result.solutions, expected);
                    } else {
                        expected = Some(result.solutions.clone());
                    }
                    if let Some(nodes) = expected_nodes {
                        assert_eq!(result.nodes, nodes);
                    } else {
                        expected_nodes = Some(result.nodes);
                    }
                    if let Some(hits) = expected_hits {
                        assert_eq!(stats.hits, hits);
                    } else {
                        expected_hits = Some(stats.hits);
                    }
                    println!(
                        "TIMING_BATCH set={} case={} round={} layout={} passes={}",
                        names, case, round, layout, timing_passes
                    );
                    println!(
                        "MATCH set={} case={} round={} layout={} ms={:.3} cpu_ms={:.3} nodes={} queries={} rejected={} exact_probes={} hits={}",
                        names,
                        case,
                        round,
                        layout,
                        ms,
                        cpu,
                        result.nodes,
                        stats.queries,
                        stats.rejected,
                        stats.exact_probes,
                        stats.hits
                    );
                }
            }
        }
        let coord = micro_start.unwrap();
        let mut collector = QueryCollector {
            tables: &tables,
            pruning: &pruning,
            moves: &moves,
            commute,
            queries: Vec::new(),
        };
        collector.visit(
            SearchState::from_coord(coord).with_pruning_indices(Some(&pruning), true, coord),
            9,
            None,
        );
        let passes = 5;
        let mut expected_hits = None;
        for round in 0..=repeats {
            let mut order = vec![0_usize, 1, 2];
            order.rotate_left(round % 3);
            if round % 2 == 1 {
                order.reverse();
            }
            for mode in order {
                let layout = ["baseline", "standard", "blocked"][mode];
                let filter = mode.checked_sub(1).map(|i| &filters[i]);
                let cpu = cpu_ms();
                let start = Instant::now();
                let stats = probes(&collector.queries, &back.paths, filter, passes);
                let ms = start.elapsed().as_secs_f64() * 1000.;
                let cpu = cpu_ms() - cpu;
                if let Some(hits) = expected_hits {
                    assert_eq!(stats.hits, hits);
                } else {
                    expected_hits = Some(stats.hits);
                }
                println!(
                    "LOOKUP set={} round={} layout={} ms={:.3} cpu_ms={:.3} queries={} rejected={} exact_probes={} hits={} passes={}",
                    names,
                    round,
                    layout,
                    ms,
                    cpu,
                    stats.queries,
                    stats.rejected,
                    stats.exact_probes,
                    stats.hits,
                    passes
                );
            }
        }
        println!("SET_DONE set={} {}", names, memory_kib());
    }
}
