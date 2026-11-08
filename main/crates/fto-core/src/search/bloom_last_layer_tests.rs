use super::*;
use crate::bloom::{BloomMode, BloomTable, GenerateConfig};
use std::collections::HashSet;

fn sorted(mut solutions: Vec<Vec<Move>>) -> Vec<Vec<Move>> {
    solutions.sort_by_key(|s| s.iter().map(|m| m.idx()).collect::<Vec<_>>());
    solutions
}

#[test]
fn last_layer_bloom_key_matches_center_goal_semantics() {
    let solved = FtoCubie::solved();
    let goal = LastLayerGoal::solved();
    let goal_key = LastLayerState::from_cubie(solved).bloom_hashes();
    let mut equivalent = 0;
    let mut distinct_same_color = 0;
    for uf in [false, true] {
        for a in 0..12 {
            for b in a + 1..12 {
                let mut cubie = solved;
                if uf {
                    cubie.uf.swap(a, b);
                } else {
                    cubie.rl.swap(a, b);
                }
                let state = LastLayerState::from_cubie(cubie);
                assert_eq!(
                    state.is_goal(goal),
                    state.bloom_hashes() == goal_key,
                    "center goal mismatch uf={uf} swap={a},{b}"
                );
                if state.is_goal(goal) {
                    equivalent += 1;
                    // Equivalent center labels remain equivalent after every move.
                    for mv in Move::ALL {
                        assert_eq!(
                            LastLayerState::from_cubie(cubie.apply(mv)).bloom_hashes(),
                            LastLayerState::from_cubie(solved.apply(mv)).bloom_hashes()
                        );
                    }
                } else if a / 3 == b / 3 {
                    distinct_same_color += 1;
                }
            }
        }
    }
    assert!(equivalent > 0);
    assert!(
        distinct_same_color > 0,
        "must preserve unique center identities within a color"
    );
}

#[test]
#[ignore = "loads the real cached transition tables"]
fn last_layer_bloom_radius_auf_threading_and_cache() {
    let tables = TransitionTables::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../cache/transition-table.tran"
    ))
    .unwrap();
    let positions = last_layer_position_moves();
    let mut alternate_goal = FtoCubie::solved();
    alternate_goal.uf.swap(0, 1);
    alternate_goal.rl.swap(4, 5);
    assert!(LastLayerState::from_cubie(alternate_goal).is_goal(LastLayerGoal::solved()));
    for moves in [
        vec![Move::R],
        vec![Move::R, Move::Rp, Move::F, Move::Fp],
        vec![Move::RURp, Move::RUpRp, Move::F],
        Move::ALL.to_vec(),
    ] {
        let table = BloomTable::generate(
            &tables,
            &moves,
            GenerateConfig {
                mode: BloomMode::LastLayer,
                depth: 3,
                max_memory_mib: 4,
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(table.mode(), BloomMode::LastLayer);
        let mut tested = HashSet::new();
        // Independent cubie expansion, with no generator canonical reductions.
        for goal in [FtoCubie::solved(), alternate_goal] {
            let mut frontier = vec![goal, goal.apply(Move::U), goal.apply(Move::Up)];
            for depth in 0..=3 {
                let mut next = Vec::new();
                for cubie in frontier {
                    let state = LastLayerState::from_cubie(cubie);
                    assert!(
                        table.contains_hashes(state.bloom_hashes()),
                        "false negative moves={moves:?} depth={depth}"
                    );
                    if depth == 3 || !tested.insert((depth, state.bloom_hashes())) {
                        continue;
                    }
                    for &mv in &moves {
                        let child = cubie.apply(mv.inverse());
                        assert_eq!(
                            state
                                .bloom_step(&tables, mv.inverse(), &positions)
                                .bloom_hashes(),
                            LastLayerState::from_cubie(child).bloom_hashes()
                        );
                        next.push(child);
                    }
                }
                frontier = next;
            }
        }
    }
    let moves = vec![Move::R, Move::Rp, Move::F, Move::Fp]; // U deliberately absent
    let mut tables_by_depth = Vec::new();
    for depth in [2, 4] {
        tables_by_depth.push(Arc::new(
            BloomTable::generate(
                &tables,
                &moves,
                GenerateConfig {
                    mode: BloomMode::LastLayer,
                    depth,
                    max_memory_mib: 1,
                },
                None,
                None,
            )
            .unwrap(),
        ));
    }
    let mut before = 0;
    let mut after = 0;
    for case in 0..4 {
        let interior = [
            moves[case % 2],
            moves[2 + (case / 2)],
            moves[(case + 1) % 2],
            moves[2 + (case % 2)],
        ];
        for prefix in [None, Some(Move::U), Some(Move::Up)] {
            for suffix in [None, Some(Move::U), Some(Move::Up)] {
                let goal = if case % 2 == 0 {
                    FtoCubie::solved()
                } else {
                    alternate_goal
                };
                let mut start = goal;
                if let Some(mv) = suffix {
                    start = start.apply(mv.inverse());
                }
                for mv in interior.into_iter().rev() {
                    start = start.apply(mv.inverse());
                }
                if let Some(mv) = prefix {
                    start = start.apply(mv.inverse());
                }
                let base = SearchConfig {
                    min_depth: 4,
                    max_depth: 4,
                    allowed_moves: moves.clone(),
                    ..Default::default()
                };
                let expected =
                    solve_last_layer_with_pattern_pruning_threads(start, &tables, None, &base, 1);
                assert!(
                    !expected.solutions.is_empty(),
                    "no baseline solution for prefix={prefix:?} suffix={suffix:?}"
                );
                for solution in &expected.solutions {
                    let end = solution.iter().fold(start, |s, &m| s.apply(m));
                    assert!(LastLayerState::from_cubie(end).is_goal(LastLayerGoal::solved()));
                }
                for bloom in &tables_by_depth {
                    let config = SearchConfig {
                        bloom: Some(bloom.clone()),
                        ..base.clone()
                    };
                    for threads in [1, 4] {
                        let actual = solve_last_layer_with_pattern_pruning_threads(
                            start, &tables, None, &config, threads,
                        );
                        assert_eq!(
                            sorted(expected.solutions.clone()),
                            sorted(actual.solutions),
                            "solution mismatch prefix={prefix:?} suffix={suffix:?} threads={threads} radius={}",
                            bloom.depth()
                        );
                        if threads == 1 && bloom.depth() == 2 {
                            before += expected.nodes;
                            after += actual.nodes;
                        }
                    }
                }
            }
        }
    }
    assert!(
        after < before,
        "last-layer Bloom table must actually prune ({after}/{before})"
    );
    println!("last-layer AUF comparison: baseline nodes={before}, Bloom nodes={after}");
    let root = std::env::temp_dir().join(format!("fto-ll-bloom-{}", std::process::id()));
    let table = &tables_by_depth[0];
    let file = crate::bloom::cache_file_name(&moves, table.depth(), table.mode());
    assert!(file.starts_with("ll-"));
    assert_ne!(
        file,
        crate::bloom::cache_file_name(&moves, table.depth(), BloomMode::FullPosition)
    );
    let path = root.join(file);
    table.save(&path, None).unwrap();
    let loaded = Arc::new(BloomTable::load(&path, None).unwrap());
    assert_eq!(
        BloomTable::read_info(&path).unwrap().mode,
        BloomMode::LastLayer
    );
    assert_eq!(loaded.mode(), BloomMode::LastLayer);
    assert_eq!(loaded.depth(), 2);
    let config = SearchConfig {
        allowed_moves: moves.clone(),
        bloom: Some(loaded.clone()),
        ..Default::default()
    };
    assert!(bloom_for_config(&config).is_none());
    assert!(last_layer_bloom_for_config(&config).is_some());
    let mut incompatible = config.clone();
    incompatible.allowed_moves.push(Move::B);
    assert!(last_layer_bloom_for_config(&incompatible).is_none());
    let full = Arc::new(
        BloomTable::generate(
            &tables,
            &moves,
            GenerateConfig {
                mode: BloomMode::FullPosition,
                depth: 2,
                max_memory_mib: 1,
            },
            None,
            None,
        )
        .unwrap(),
    );
    assert!(
        last_layer_bloom_for_config(&SearchConfig {
            bloom: Some(full),
            ..config
        })
        .is_none()
    );
    for cubie in [
        FtoCubie::solved(),
        FtoCubie::solved().apply(Move::U).apply(Move::Rp),
    ] {
        assert_eq!(
            loaded.contains_hashes(LastLayerState::from_cubie(cubie).bloom_hashes()),
            table.contains_hashes(LastLayerState::from_cubie(cubie).bloom_hashes())
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
