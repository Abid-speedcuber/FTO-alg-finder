use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process,
    time::Instant,
};

use fto_core::{
    moves::Move,
    partial::{self, PartialMask, PartialProblem},
    pruning::{self, SolverPruning, PruningStats},
    search::{self, BidirectionalConfig, SearchConfig},
    tables::TransitionTables,
    FtoCubie,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut max_depth = None;
    let mut exact_depth = false;
    let mut find_all = false;
    let mut eval_pruning = false;
    let mut auto_pruning = false;
    let mut benchmark = false;
    let mut benchmark_iters = 7_usize;
    let mut threads = 1_usize;
    let mut force_bidirectional = false;
    let mut disable_bidirectional = false;
    let mut bidirectional_threshold = 19_u8;
    let mut bidirectional_max_mib = 3072_usize;
    let mut bidirectional_start_pruning = false;
    let mut restricted_pruning = false;
    let mut allowed_moves = Move::ALL.to_vec();
    let mut use_solver_pruning = true;
    let mut keep_all_pruning_pdbs = false;
    let mut max_pruning_mib = 1024_usize;
    let mut max_pruning_components = 3_usize;
    let mut top_pruning_candidates = 12_usize;
    let mut max_combo_size = 5_usize;
    let mut sample_count = 100_000_usize;
    let mut sample_walk_len = 40_usize;
    let mut pruning_out_dir = PathBuf::from("cache/pruning-v4");
    let mut progress_million = 5_usize;
    let mut pruning_candidate = None;
    let mut json_path = None;
    let mut scramble = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--depth" => {
                i += 1;
                max_depth = Some(args
                    .get(i)
                    .ok_or("--depth needs a value")?
                    .parse()
                    .map_err(|_| "--depth must be an integer from 0 to 255")?);
            }
            "--all" => find_all = true,
            "--exact" => exact_depth = true,
            "--eval-pruning" => eval_pruning = true,
            "--auto-pruning" => auto_pruning = true,
            "--benchmark" => benchmark = true,
            "--bidirectional" => force_bidirectional = true,
            "--no-bidirectional" => disable_bidirectional = true,
            "--bidir-start-pruning" => bidirectional_start_pruning = true,
            "--restricted-pruning" => restricted_pruning = true,
            "--no-pruning" => use_solver_pruning = false,
            "--keep-all-pruning-pdbs" => keep_all_pruning_pdbs = true,
            "--benchmark-iters" => {
                i += 1;
                benchmark_iters = args
                    .get(i)
                    .ok_or("--benchmark-iters needs a value")?
                    .parse()
                    .map_err(|_| "--benchmark-iters must be an integer")?;
            }
            "--threads" => {
                i += 1;
                threads = args
                    .get(i)
                    .ok_or("--threads needs a value")?
                    .parse()
                    .map_err(|_| "--threads must be an integer")?;
                if threads == 0 {
                    return Err("--threads must be at least 1".to_owned());
                }
            }
            "--bidir-threshold" => {
                i += 1;
                bidirectional_threshold = args
                    .get(i)
                    .ok_or("--bidir-threshold needs a value")?
                    .parse()
                    .map_err(|_| "--bidir-threshold must be an integer from 0 to 255")?;
            }
            "--bidir-max-mib" => {
                i += 1;
                bidirectional_max_mib = args
                    .get(i)
                    .ok_or("--bidir-max-mib needs a value")?
                    .parse()
                    .map_err(|_| "--bidir-max-mib must be an integer")?;
            }
            "--moves" => {
                i += 1;
                allowed_moves = parse_move_list(args.get(i).ok_or("--moves needs a move list")?)?;
            }
            "--ban" => {
                i += 1;
                let banned = parse_move_list(args.get(i).ok_or("--ban needs a move list")?)?;
                allowed_moves.retain(|mv| !banned.contains(mv));
                if allowed_moves.is_empty() {
                    return Err("--ban removed every move".to_owned());
                }
            }
            "--max-pruning-mib" => {
                i += 1;
                max_pruning_mib = args
                    .get(i)
                    .ok_or("--max-pruning-mib needs a value")?
                    .parse()
                    .map_err(|_| "--max-pruning-mib must be an integer")?;
            }
            "--max-pruning-components" => {
                i += 1;
                max_pruning_components = args
                    .get(i)
                    .ok_or("--max-pruning-components needs a value")?
                    .parse()
                    .map_err(|_| "--max-pruning-components must be an integer")?;
            }
            "--top-pruning-candidates" => {
                i += 1;
                top_pruning_candidates = args
                    .get(i)
                    .ok_or("--top-pruning-candidates needs a value")?
                    .parse()
                    .map_err(|_| "--top-pruning-candidates must be an integer")?;
            }
            "--max-combo-size" => {
                i += 1;
                max_combo_size = args
                    .get(i)
                    .ok_or("--max-combo-size needs a value")?
                    .parse()
                    .map_err(|_| "--max-combo-size must be an integer")?;
            }
            "--sample-count" => {
                i += 1;
                sample_count = args
                    .get(i)
                    .ok_or("--sample-count needs a value")?
                    .parse()
                    .map_err(|_| "--sample-count must be an integer")?;
            }
            "--sample-walk-len" => {
                i += 1;
                sample_walk_len = args
                    .get(i)
                    .ok_or("--sample-walk-len needs a value")?
                    .parse()
                    .map_err(|_| "--sample-walk-len must be an integer")?;
            }
            "--pruning-out-dir" => {
                i += 1;
                pruning_out_dir = PathBuf::from(
                    args.get(i)
                        .ok_or("--pruning-out-dir needs a directory path")?,
                );
            }
            "--candidate" => {
                i += 1;
                pruning_candidate = Some(args.get(i).ok_or("--candidate needs a value")?.clone());
            }
            "--progress-million" => {
                i += 1;
                progress_million = args
                    .get(i)
                    .ok_or("--progress-million needs a value")?
                    .parse()
                    .map_err(|_| "--progress-million must be an integer")?;
            }
            "--json" => {
                i += 1;
                json_path = Some(args.get(i).ok_or("--json needs a file path")?.clone());
            }
            "--scramble" => {
                i += 1;
                scramble = Some(args.get(i).ok_or("--scramble needs a quoted move sequence")?.clone());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    eprintln!("loading transition tables...");
    let tables = TransitionTables::load_or_build("cache/transition-tables-v4.bin")
        .map_err(|error| error.to_string())?;

    if auto_pruning {
        let max_entries = max_pruning_mib
            .checked_mul(1024 * 1024)
            .ok_or("--max-pruning-mib is too large")?;
        run_auto_pruning(
            &tables,
            &pruning_out_dir,
            max_entries,
            max_pruning_components,
            progress_million * 1_000_000,
            sample_count,
            sample_walk_len,
            top_pruning_candidates,
            max_combo_size,
            keep_all_pruning_pdbs,
        )?;
        return Ok(());
    }

    if eval_pruning {
        let max_entries = max_pruning_mib
            .checked_mul(1024 * 1024)
            .ok_or("--max-pruning-mib is too large")?;
        println!("candidate,entries,bytes,reached,max_depth,avg_depth,histogram");
        let mut stats = Vec::new();
        let candidates = if let Some(candidate) = pruning_candidate {
            vec![parse_candidate(&candidate)?]
        } else {
            pruning::default_candidates()
        };
        for candidate in candidates {
            eprintln!(
                "evaluating {} ({:.1} MiB table)",
                candidate.name(),
                candidate.size().unwrap_or(0) as f64 / (1024.0 * 1024.0)
            );
            match pruning::evaluate_candidate_with_progress(
                &candidate,
                &tables,
                max_entries,
                progress_million * 1_000_000,
            ) {
                Ok(candidate_stats) => {
                    print_stats(&candidate_stats);
                    stats.push(candidate_stats);
                }
                Err(error) => eprintln!("skip: {error}"),
            }
        }
        stats.sort_by(|a, b| {
            b.average_depth_milli
                .cmp(&a.average_depth_milli)
                .then_with(|| a.table_bytes.cmp(&b.table_bytes))
        });
        eprintln!("best by average depth:");
        for stat in stats.iter().take(10) {
            eprintln!(
                "{}: avg {:.3}, max {}, {:.1} MiB, reached {}",
                stat.name,
                f64::from(stat.average_depth_milli) / 1000.0,
                stat.max_depth,
                stat.table_bytes as f64 / (1024.0 * 1024.0),
                stat.reached
            );
        }
        stats.sort_by(|a, b| {
            roi_milli(b)
                .cmp(&roi_milli(a))
                .then_with(|| b.average_depth_milli.cmp(&a.average_depth_milli))
        });
        eprintln!("best by average-depth per MiB:");
        for stat in stats.iter().take(10) {
            eprintln!(
                "{}: roi {:.3}, avg {:.3}, max {}, {:.1} MiB",
                stat.name,
                roi_milli(stat) as f64 / 1000.0,
                f64::from(stat.average_depth_milli) / 1000.0,
                stat.max_depth,
                stat.table_bytes as f64 / (1024.0 * 1024.0),
            );
        }
        return Ok(());
    }

    let input_state = if let Some(sequence) = scramble {
        InputState::Exact(apply_sequence(FtoCubie::solved(), &sequence)?)
    } else {
        let input = if let Some(path) = json_path {
            fs::read_to_string(path).map_err(|error| error.to_string())?
        } else {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|error| error.to_string())?;
            input
        };
        if input.trim().is_empty() {
            InputState::Exact(FtoCubie::solved())
        } else {
            parse_input_json(&input)?
        }
    };

    if let InputState::Partial(problem) = input_state {
        let result = solve_partial_input(
            &problem,
            max_depth,
            exact_depth,
            find_all,
            allowed_moves,
        );
        println!("nodes: {}", result.nodes);
        println!("solutions: {}", result.solutions.len());
        for solution in result.solutions {
            println!("{}", search::format_solution(&solution));
        }
        return Ok(());
    }

    let InputState::Exact(cubie) = input_state else {
        unreachable!();
    };

    let pruning_tables = if use_solver_pruning {
        Some(load_solver_pruning(
            &tables,
            &pruning_out_dir,
            progress_million * 1_000_000,
            if restricted_pruning {
                &allowed_moves
            } else {
                &Move::ALL
            },
        )?)
    } else {
        None
    };
    let coord = cubie.coord();
    if benchmark {
        run_solve_benchmark(
            coord,
            &tables,
            pruning_tables.as_ref(),
            max_depth,
            exact_depth,
            find_all,
            benchmark_iters,
            threads,
            bidirectional_policy(force_bidirectional, disable_bidirectional, bidirectional_threshold),
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_million * 1_000_000,
            allowed_moves.clone(),
        )?;
        return Ok(());
    }
    let result = if let Some(max_depth) = max_depth {
        solve_once(
            coord,
            &tables,
            pruning_tables.as_ref(),
            max_depth,
            exact_depth,
            find_all,
            threads,
            bidirectional_policy(force_bidirectional, disable_bidirectional, bidirectional_threshold),
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_million * 1_000_000,
            allowed_moves.clone(),
        )
    } else {
        solve_incrementally(
            coord,
            &tables,
            pruning_tables.as_ref(),
            find_all,
            threads,
            bidirectional_policy(force_bidirectional, disable_bidirectional, bidirectional_threshold),
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_million * 1_000_000,
            allowed_moves,
        )
    }?;

    println!("nodes: {}", result.nodes);
    println!("solutions: {}", result.solutions.len());
    for solution in result.solutions {
        println!("{}", search::format_solution(&solution));
    }
    Ok(())
}

fn solve_partial_input(
    problem: &PartialProblem,
    max_depth: Option<u8>,
    exact_depth: bool,
    find_all: bool,
    allowed_moves: Vec<Move>,
) -> search::SearchResult {
    let config = SearchConfig {
        min_depth: if exact_depth {
            max_depth.unwrap_or(0)
        } else {
            0
        },
        max_depth: max_depth.unwrap_or(u8::MAX),
        find_all,
        allowed_moves,
        cancel: None,
    };
    partial::solve_partial(problem, &config)
}

fn solve_once(
    coord: fto_core::FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    max_depth: u8,
    exact_depth: bool,
    find_all: bool,
    threads: usize,
    bidirectional_threshold: Option<u8>,
    bidirectional_max_mib: usize,
    bidirectional_start_pruning: bool,
    progress_interval: usize,
    allowed_moves: Vec<Move>,
) -> Result<search::SearchResult, String> {
    if exact_depth
        && bidirectional_threshold.is_some_and(|threshold| max_depth >= threshold)
    {
        eprintln!("using bidirectional exact-depth search");
        return search::solve_bidirectional(
            coord,
            tables,
            pruning,
            &BidirectionalConfig {
                depth: max_depth,
                find_all,
                max_stored_paths: bidirectional_max_paths(bidirectional_max_mib),
                threads,
                use_start_pruning: bidirectional_start_pruning,
                progress_interval,
                allowed_moves: allowed_moves.clone(),
            },
        );
    }

    Ok(search::solve_with_pruning_threads(
        coord,
        tables,
        pruning,
        &SearchConfig {
            min_depth: if exact_depth { max_depth } else { 0 },
            max_depth,
            find_all,
            allowed_moves,
            cancel: None,
        },
        threads,
    ))
}

fn bidirectional_policy(force: bool, disable: bool, threshold: u8) -> Option<u8> {
    if disable {
        None
    } else if force {
        Some(0)
    } else {
        Some(threshold)
    }
}

fn bidirectional_max_paths(max_mib: usize) -> usize {
    max_mib.saturating_mul(1024 * 1024) / 96
}

fn run_solve_benchmark(
    coord: fto_core::FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    max_depth: Option<u8>,
    exact_depth: bool,
    find_all: bool,
    iterations: usize,
    threads: usize,
    bidirectional_threshold: Option<u8>,
    bidirectional_max_mib: usize,
    bidirectional_start_pruning: bool,
    progress_interval: usize,
    allowed_moves: Vec<Move>,
) -> Result<(), String> {
    let iterations = iterations.max(1);
    let warmups = 2;
    for _ in 0..warmups {
        let result = benchmark_solve_once(
            coord,
            tables,
            pruning,
            max_depth,
            exact_depth,
            find_all,
            threads,
            bidirectional_threshold,
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_interval,
            allowed_moves.clone(),
        )?;
        std::hint::black_box(result.nodes);
        std::hint::black_box(result.solutions.len());
    }

    let mut samples = Vec::with_capacity(iterations);
    let mut last_result = None;
    for _ in 0..iterations {
        let start = Instant::now();
        let result = benchmark_solve_once(
            coord,
            tables,
            pruning,
            max_depth,
            exact_depth,
            find_all,
            threads,
            bidirectional_threshold,
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_interval,
            allowed_moves.clone(),
        )?;
        let elapsed = start.elapsed();
        std::hint::black_box(result.nodes);
        std::hint::black_box(result.solutions.len());
        samples.push(elapsed.as_nanos() as u64);
        last_result = Some(result);
    }
    samples.sort_unstable();
    let median_ns = samples[samples.len() / 2];
    let mean_ns = samples.iter().sum::<u64>() / samples.len() as u64;
    let result = last_result.expect("at least one benchmark iteration");
    let median_nodes_per_sec = if median_ns == 0 {
        0
    } else {
        result.nodes.saturating_mul(1_000_000_000) / median_ns
    };
    let median_ns_per_node = if result.nodes == 0 {
        0.0
    } else {
        median_ns as f64 / result.nodes as f64
    };

    println!("benchmark_iterations: {iterations}");
    println!("nodes: {}", result.nodes);
    println!("solutions: {}", result.solutions.len());
    println!("median_ms: {:.3}", median_ns as f64 / 1_000_000.0);
    println!("mean_ms: {:.3}", mean_ns as f64 / 1_000_000.0);
    println!("median_nodes_per_sec: {median_nodes_per_sec}");
    println!("median_ns_per_node: {:.1}", median_ns_per_node);
    if let Some(solution) = result.solutions.first() {
        println!("first_solution: {}", search::format_solution(solution));
    }
    Ok(())
}

fn benchmark_solve_once(
    coord: fto_core::FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    max_depth: Option<u8>,
    exact_depth: bool,
    find_all: bool,
    threads: usize,
    bidirectional_threshold: Option<u8>,
    bidirectional_max_mib: usize,
    bidirectional_start_pruning: bool,
    progress_interval: usize,
    allowed_moves: Vec<Move>,
) -> Result<search::SearchResult, String> {
    if let Some(max_depth) = max_depth {
        solve_once(
            coord,
            tables,
            pruning,
            max_depth,
            exact_depth,
            find_all,
            threads,
            bidirectional_threshold,
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_interval,
            allowed_moves,
        )
    } else {
        solve_incrementally_quiet(
            coord,
            tables,
            pruning,
            find_all,
            threads,
            bidirectional_threshold,
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_interval,
            allowed_moves,
        )
    }
}

fn load_solver_pruning(
    tables: &TransitionTables,
    out_dir: &Path,
    progress_interval: usize,
    moves: &[Move],
) -> Result<SolverPruning, String> {
    let pruning = SolverPruning::load_or_build_with_moves(tables, out_dir, progress_interval, moves)?;
    eprintln!(
        "using 2 pruning tables: edge3+uf3 / corner+uf3 ({:.1} MiB)",
        pruning.total_bytes() as f64 / (1024.0 * 1024.0)
    );
    Ok(pruning)
}

fn solve_incrementally(
    coord: fto_core::FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    find_all: bool,
    threads: usize,
    bidirectional_threshold: Option<u8>,
    bidirectional_max_mib: usize,
    bidirectional_start_pruning: bool,
    progress_interval: usize,
    allowed_moves: Vec<Move>,
) -> Result<search::SearchResult, String> {
    let start_depth = pruning
        .map(|tables| tables.heuristic_for_coord(coord))
        .unwrap_or(0);
    let mut total_nodes = 0_u64;
    for depth in start_depth..=u8::MAX {
        eprintln!("searching depth {depth}...");
        let mut result = solve_once(
            coord,
            tables,
            pruning,
            depth,
            true,
            find_all,
            threads,
            bidirectional_threshold,
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_interval,
            allowed_moves.clone(),
        )?;
        total_nodes += result.nodes;
        if !result.solutions.is_empty() {
            result.nodes = total_nodes;
            eprintln!("found solution at depth {depth}");
            return Ok(result);
        }
    }
    Err("no solution found up to depth 255".to_owned())
}

fn solve_incrementally_quiet(
    coord: fto_core::FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    find_all: bool,
    threads: usize,
    bidirectional_threshold: Option<u8>,
    bidirectional_max_mib: usize,
    bidirectional_start_pruning: bool,
    progress_interval: usize,
    allowed_moves: Vec<Move>,
) -> Result<search::SearchResult, String> {
    let start_depth = pruning
        .map(|tables| tables.heuristic_for_coord(coord))
        .unwrap_or(0);
    let mut total_nodes = 0_u64;
    for depth in start_depth..=u8::MAX {
        let mut result = solve_once(
            coord,
            tables,
            pruning,
            depth,
            true,
            find_all,
            threads,
            bidirectional_threshold,
            bidirectional_max_mib,
            bidirectional_start_pruning,
            progress_interval,
            allowed_moves.clone(),
        )?;
        total_nodes += result.nodes;
        if !result.solutions.is_empty() {
            result.nodes = total_nodes;
            return Ok(result);
        }
    }
    Err("no solution found up to depth 255".to_owned())
}

fn print_stats(stats: &PruningStats) {
    println!(
        "{},{},{},{},{},{:.3},{}",
        stats.name,
        stats.table_entries,
        stats.table_bytes,
        stats.reached,
        stats.max_depth,
        f64::from(stats.average_depth_milli) / 1000.0,
        stats
            .histogram
            .iter()
            .enumerate()
            .map(|(depth, count)| format!("{depth}:{count}"))
            .collect::<Vec<_>>()
            .join("|")
    );
}

fn run_auto_pruning(
    tables: &TransitionTables,
    out_dir: &Path,
    max_entries: usize,
    max_components: usize,
    progress_interval: usize,
    sample_count: usize,
    sample_walk_len: usize,
    top_candidates: usize,
    max_combo_size: usize,
    keep_all_pdbs: bool,
) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
    let mut stats_file =
        File::create(out_dir.join("stats.csv")).map_err(|error| error.to_string())?;
    writeln!(
        stats_file,
        "candidate,entries,bytes,reached,max_depth,avg_depth,histogram,file"
    )
    .map_err(|error| error.to_string())?;

    let candidates = pruning::generated_candidates(max_components, max_entries);
    eprintln!(
        "auto pruning: {} candidates, cap {:.1} MiB, output {}",
        candidates.len(),
        max_entries as f64 / (1024.0 * 1024.0),
        out_dir.display()
    );

    let mut generated = Vec::new();
    for candidate in candidates {
        let name = candidate.name();
        let path = out_dir.join(format!("{}.pdb", file_safe_name(&name)));
        eprintln!(
            "building {} ({:.1} MiB)",
            name,
            candidate.size().unwrap_or(0) as f64 / (1024.0 * 1024.0)
        );
        let stats = pruning::build_and_save_candidate(
            &candidate,
            tables,
            max_entries,
            progress_interval,
            &path,
        )?;
        writeln!(
            stats_file,
            "{},{},{},{},{},{:.3},{},{}",
            stats.name,
            stats.table_entries,
            stats.table_bytes,
            stats.reached,
            stats.max_depth,
            f64::from(stats.average_depth_milli) / 1000.0,
            histogram_string(&stats),
            path.display()
        )
        .map_err(|error| error.to_string())?;
        stats_file.flush().map_err(|error| error.to_string())?;
        print_stats(&stats);
        generated.push(GeneratedCandidate {
            spec: candidate,
            stats,
            path,
        });
    }

    generated.sort_by(|a, b| {
        b.stats
            .average_depth_milli
            .cmp(&a.stats.average_depth_milli)
            .then_with(|| a.stats.table_bytes.cmp(&b.stats.table_bytes))
    });
    let selected = generated
        .into_iter()
        .take(top_candidates)
        .collect::<Vec<_>>();
    let selected_paths = selected
        .iter()
        .map(|candidate| candidate.path.clone())
        .collect::<HashSet<_>>();
    eprintln!("sampling {sample_count} states with walks up to {sample_walk_len} moves");
    let samples = sample_states(tables, sample_count, sample_walk_len);
    eprintln!("loading sampled values for {} top candidates", selected.len());
    let sampled = selected
        .iter()
        .map(|candidate| load_sampled_values(candidate, &samples))
        .collect::<Result<Vec<_>, _>>()?;

    let mut combo_file =
        File::create(out_dir.join("combos.csv")).map_err(|error| error.to_string())?;
    writeln!(
        combo_file,
        "tables,count,total_bytes,avg_max,max_seen,histogram"
    )
    .map_err(|error| error.to_string())?;
    let mut summaries = Vec::new();
    let max_width = max_combo_size.min(sampled.len());
    for width in 1..=max_width {
        score_combinations(
            &sampled,
            width,
            0,
            &mut Vec::new(),
            &mut combo_file,
            &mut summaries,
        )?;
    }
    combo_file.flush().map_err(|error| error.to_string())?;

    summaries.sort_by(|a, b| {
        b.avg_max_milli
            .cmp(&a.avg_max_milli)
            .then_with(|| a.total_bytes.cmp(&b.total_bytes))
    });
    eprintln!("best sampled combinations:");
    for summary in summaries.iter().take(20) {
        eprintln!(
            "{}: avg max {:.3}, max {}, {:.1} MiB",
            summary.names.join(" / "),
            summary.avg_max_milli as f64 / 1000.0,
            summary.max_seen,
            summary.total_bytes as f64 / (1024.0 * 1024.0)
        );
    }
    if keep_all_pdbs {
        eprintln!("keeping all generated pruning tables");
    } else {
        let removed = cleanup_unselected_pdbs(out_dir, &selected_paths)?;
        eprintln!("removed {removed} unselected pruning tables from {}", out_dir.display());
    }
    Ok(())
}

fn cleanup_unselected_pdbs(out_dir: &Path, keep_paths: &HashSet<PathBuf>) -> Result<usize, String> {
    let mut removed = 0;
    for entry in fs::read_dir(out_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "pdb") && !keep_paths.contains(&path)
        {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
            removed += 1;
        }
    }
    Ok(removed)
}

struct GeneratedCandidate {
    spec: pruning::CandidateSpec,
    stats: PruningStats,
    path: PathBuf,
}

struct SampledCandidate {
    name: String,
    bytes: usize,
    values: Vec<u8>,
}

struct ComboSummary {
    names: Vec<String>,
    total_bytes: usize,
    avg_max_milli: u64,
    max_seen: u8,
}

fn sample_states(
    tables: &TransitionTables,
    sample_count: usize,
    sample_walk_len: usize,
) -> Vec<fto_core::FtoCoord> {
    let mut rng = Lcg::new(0x6a09_e667_f3bc_c909);
    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let mut coord = fto_core::FtoCoord::solved();
        let depth = 1 + rng.next_usize(sample_walk_len.max(1));
        let mut last_axis = None;
        for _ in 0..depth {
            let mv = loop {
                let candidate = fto_core::moves::Move::ALL[rng.next_usize(fto_core::moves::MOVE_COUNT)];
                if last_axis != Some(candidate.axis()) {
                    break candidate;
                }
            };
            coord = tables.apply(coord, mv);
            last_axis = Some(mv.axis());
        }
        samples.push(coord);
    }
    samples
}

fn load_sampled_values(
    candidate: &GeneratedCandidate,
    samples: &[fto_core::FtoCoord],
) -> Result<SampledCandidate, String> {
    eprintln!("  sampling {}", candidate.stats.name);
    let mut file = File::open(&candidate.path).map_err(|error| error.to_string())?;
    let mut values = Vec::with_capacity(samples.len());
    let mut byte = [0_u8; 1];
    for &sample in samples {
        let idx = candidate.spec.index_of_coord(sample);
        file.seek(SeekFrom::Start(idx as u64))
            .map_err(|error| error.to_string())?;
        file.read_exact(&mut byte)
            .map_err(|error| error.to_string())?;
        values.push(if byte[0] == u8::MAX { 0 } else { byte[0] });
    }
    Ok(SampledCandidate {
        name: candidate.stats.name.clone(),
        bytes: candidate.stats.table_bytes,
        values,
    })
}

fn score_combinations(
    candidates: &[SampledCandidate],
    width: usize,
    start: usize,
    current: &mut Vec<usize>,
    out: &mut File,
    summaries: &mut Vec<ComboSummary>,
) -> Result<(), String> {
    if current.len() == width {
        let mut histogram = Vec::<usize>::new();
        let mut sum = 0_u64;
        let mut max_seen = 0_u8;
        let sample_len = candidates[current[0]].values.len();
        for sample_idx in 0..sample_len {
            let value = current
                .iter()
                .map(|&candidate_idx| candidates[candidate_idx].values[sample_idx])
                .max()
                .unwrap_or(0);
            sum += u64::from(value);
            max_seen = max_seen.max(value);
            let hist_idx = usize::from(value);
            if histogram.len() <= hist_idx {
                histogram.resize(hist_idx + 1, 0);
            }
            histogram[hist_idx] += 1;
        }
        let names = current
            .iter()
            .map(|&idx| candidates[idx].name.clone())
            .collect::<Vec<_>>();
        let total_bytes = current.iter().map(|&idx| candidates[idx].bytes).sum::<usize>();
        let avg_max_milli = sum * 1000 / sample_len as u64;
        writeln!(
            out,
            "{},{},{},{:.3},{},{}",
            names.join(" / "),
            width,
            total_bytes,
            avg_max_milli as f64 / 1000.0,
            max_seen,
            histogram
                .iter()
                .enumerate()
                .map(|(depth, count)| format!("{depth}:{count}"))
                .collect::<Vec<_>>()
                .join("|")
        )
        .map_err(|error| error.to_string())?;
        summaries.push(ComboSummary {
            names,
            total_bytes,
            avg_max_milli,
            max_seen,
        });
        return Ok(());
    }

    for i in start..candidates.len() {
        current.push(i);
        score_combinations(candidates, width, i + 1, current, out, summaries)?;
        current.pop();
    }
    Ok(())
}

struct Lcg {
    state: u64,
}

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.state
    }

    fn next_usize(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

fn file_safe_name(name: &str) -> String {
    name.replace('+', "__")
}

fn histogram_string(stats: &PruningStats) -> String {
    stats
        .histogram
        .iter()
        .enumerate()
        .map(|(depth, count)| format!("{depth}:{count}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn roi_milli(stats: &PruningStats) -> u64 {
    let mib_milli = ((stats.table_bytes as u64) * 1000).div_ceil(1024 * 1024);
    if mib_milli == 0 {
        return u64::from(stats.average_depth_milli) * 1000;
    }
    u64::from(stats.average_depth_milli) * 1000 / mib_milli
}

fn parse_candidate(input: &str) -> Result<pruning::CandidateSpec, String> {
    let components = input
        .split('+')
        .map(|name| {
            pruning::Component::parse(name)
                .ok_or_else(|| format!("unknown pruning component: {name}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.is_empty() {
        return Err("--candidate cannot be empty".to_owned());
    }
    Ok(pruning::CandidateSpec::new(components))
}

fn print_help() {
    println!(
        "Usage:
  fto-cli --scramble \"R U R'\"
  fto-cli --json state.json --benchmark --benchmark-iters 9
  fto-cli --json state.json --depth 19 --exact --bidirectional --bidir-max-mib 3072
  fto-cli --json state.json --depth 19 --exact --bidirectional --bidir-start-pruning
  fto-cli --json state.json --depth 17 --exact --threads 2
  fto-cli --scramble \"R U R'\" --depth 10 --ban \"D D' F Fw\"
  fto-cli --scramble \"R U R'\" --depth 10 --moves \"R R' U U'\" --restricted-pruning
  fto-cli --scramble \"R U R'\" --depth 6
  fto-cli --scramble \"R U R'\" --depth 6 --no-pruning
  fto-cli --json state.json --depth 6 --all --exact
  fto-cli --depth 6 --all --exact < state.json
  fto-cli --eval-pruning --max-pruning-mib 512
  fto-cli --eval-pruning --candidate e0+uf --max-pruning-mib 512
  fto-cli --eval-pruning --candidate e0+uf --progress-million 1
  fto-cli --eval-pruning --candidate edge3+uf3 --max-pruning-mib 1536
  fto-cli --eval-pruning --candidate edge4+uf2 --max-pruning-mib 512
  fto-cli --auto-pruning --max-pruning-mib 1024 --max-pruning-components 3
  fto-cli --auto-pruning --max-pruning-mib 1024 --keep-all-pruning-pdbs

If no depth is provided, search starts at the pruning lower bound and increases until a solution is found.
Exact searches at depth 19+ use bidirectional search unless --no-bidirectional is used.
--bidir-start-pruning builds temporary start-centered tables for the backward half.
--ban removes moves from search. --moves replaces the search move set.
By default restricted searches still use the all-move base pruning table; --restricted-pruning builds PDBs for the allowed set.
Auto pruning keeps only the top sampled candidate PDBs unless --keep-all-pruning-pdbs is used.
If no state is provided, the solved state is used."
    );
}

enum InputState {
    Exact(FtoCubie),
    Partial(PartialProblem),
}

fn parse_input_json(input: &str) -> Result<InputState, String> {
    let cubie = parse_cubie_json(input)?;
    if input.contains("\"partialMask\"") {
        Ok(InputState::Partial(PartialProblem {
            cubie,
            mask: parse_partial_mask(input)?,
        }))
    } else {
        Ok(InputState::Exact(cubie))
    }
}

fn parse_cubie_json(input: &str) -> Result<FtoCubie, String> {
    Ok(FtoCubie::new(
        parse_array::<6>(input, "cp", 0, 5)?,
        parse_array::<6>(input, "co", 0, 1)?,
        parse_array::<12>(input, "ep", 0, 11)?,
        parse_array::<12>(input, "uf", 0, 11)?,
        parse_array::<12>(input, "rl", 0, 11)?,
    ))
}

fn parse_partial_mask(input: &str) -> Result<PartialMask, String> {
    let mask = object_body(input, "partialMask")?;
    Ok(PartialMask {
        corners: parse_bool_array::<6>(mask, "corners")?,
        edges: parse_bool_array::<12>(mask, "edges")?,
        uf_centers: parse_bool_array::<12>(mask, "ufCenters")?,
        rl_centers: parse_bool_array::<12>(mask, "rlCenters")?,
    })
}

fn object_body<'a>(input: &'a str, key: &str) -> Result<&'a str, String> {
    let needle = format!("\"{key}\"");
    let key_start = input
        .find(&needle)
        .ok_or_else(|| format!("missing JSON key: {key}"))?;
    let after_key = &input[key_start + needle.len()..];
    let open = after_key
        .find('{')
        .ok_or_else(|| format!("{key} must be a JSON object"))?;
    let body_start = key_start + needle.len() + open + 1;
    let mut depth = 1_i32;
    for (offset, ch) in input[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&input[body_start..body_start + offset]);
                }
            }
            _ => {}
        }
    }
    Err(format!("{key} object is missing closing brace"))
}

fn parse_bool_array<const N: usize>(input: &str, key: &str) -> Result<[bool; N], String> {
    let needle = format!("\"{key}\"");
    let key_start = input
        .find(&needle)
        .ok_or_else(|| format!("missing JSON key: {key}"))?;
    let after_key = &input[key_start + needle.len()..];
    let open = after_key
        .find('[')
        .ok_or_else(|| format!("{key} must be a JSON array"))?;
    let after_open = &after_key[open + 1..];
    let close = after_open
        .find(']')
        .ok_or_else(|| format!("{key} array is missing closing bracket"))?;
    let values = after_open[..close]
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| match value {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!("{key} contains non-boolean value: {value}")),
        })
        .collect::<Result<Vec<_>, _>>()?;

    values
        .try_into()
        .map_err(|values: Vec<bool>| format!("{key} must contain {N} values, got {}", values.len()))
}

fn parse_array<const N: usize>(
    input: &str,
    key: &str,
    min: u8,
    max: u8,
) -> Result<[u8; N], String> {
    let needle = format!("\"{key}\"");
    let key_start = input
        .find(&needle)
        .ok_or_else(|| format!("missing JSON key: {key}"))?;
    let after_key = &input[key_start + needle.len()..];
    let open = after_key
        .find('[')
        .ok_or_else(|| format!("{key} must be a JSON array"))?;
    let after_open = &after_key[open + 1..];
    let close = after_open
        .find(']')
        .ok_or_else(|| format!("{key} array is missing closing bracket"))?;
    let body = &after_open[..close];
    let values = body
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<u8>()
                .map_err(|_| format!("{key} contains a non-u8 value: {value}"))
                .and_then(|value| {
                    if (min..=max).contains(&value) {
                        Ok(value)
                    } else {
                        Err(format!("{key} value {value} is outside {min}..={max}"))
                    }
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let out: [u8; N] = values
        .try_into()
        .map_err(|values: Vec<u8>| format!("{key} must contain {N} values, got {}", values.len()))?;
    Ok(out)
}

fn apply_sequence(mut cubie: FtoCubie, sequence: &str) -> Result<FtoCubie, String> {
    for token in sequence.split_whitespace() {
        let mv = parse_move(token)?;
        cubie = cubie.apply(mv);
    }
    Ok(cubie)
}

fn parse_move(token: &str) -> Result<fto_core::moves::Move, String> {
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

fn parse_move_list(input: &str) -> Result<Vec<Move>, String> {
    let mut moves = Vec::new();
    for token in input
        .split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .filter(|token| !token.is_empty())
    {
        let mv = parse_move(token)?;
        if !moves.contains(&mv) {
            moves.push(mv);
        }
    }
    if moves.is_empty() {
        return Err("move list cannot be empty".to_owned());
    }
    Ok(moves)
}
