use std::{
    env,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process,
};

use fto_core::{
    pruning::{self, PatternDatabase, PatternDatabases, PruningStats},
    search::{self, SearchConfig},
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
    let mut max_depth = 6_u8;
    let mut exact_depth = false;
    let mut find_all = false;
    let mut eval_pruning = false;
    let mut auto_pruning = false;
    let mut use_solver_pruning = true;
    let mut max_pruning_mib = 1024_usize;
    let mut max_pruning_components = 3_usize;
    let mut top_pruning_candidates = 12_usize;
    let mut max_combo_size = 5_usize;
    let mut sample_count = 100_000_usize;
    let mut sample_walk_len = 40_usize;
    let mut pruning_out_dir = PathBuf::from("cache/pruning");
    let mut progress_million = 5_usize;
    let mut pruning_candidate = None;
    let mut json_path = None;
    let mut scramble = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--depth" => {
                i += 1;
                max_depth = args
                    .get(i)
                    .ok_or("--depth needs a value")?
                    .parse()
                    .map_err(|_| "--depth must be an integer from 0 to 255")?;
            }
            "--all" => find_all = true,
            "--exact" => exact_depth = true,
            "--eval-pruning" => eval_pruning = true,
            "--auto-pruning" => auto_pruning = true,
            "--no-pruning" => use_solver_pruning = false,
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
    let tables = TransitionTables::load_or_build("cache/transition-tables-v3.bin")
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

    let cubie = if let Some(sequence) = scramble {
        apply_sequence(FtoCubie::solved(), &sequence)?
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
            FtoCubie::solved()
        } else {
            parse_cubie_json(&input)?
        }
    };

    let config = SearchConfig {
        min_depth: if exact_depth { max_depth } else { 0 },
        max_depth,
        find_all,
    };
    let pruning_tables = if use_solver_pruning {
        Some(load_solver_pruning(
            &tables,
            &pruning_out_dir,
            progress_million * 1_000_000,
        )?)
    } else {
        None
    };
    let result =
        search::solve_with_pruning(cubie.coord(), &tables, pruning_tables.as_ref(), &config);

    println!("nodes: {}", result.nodes);
    println!("solutions: {}", result.solutions.len());
    for solution in result.solutions {
        println!("{}", search::format_solution(&solution));
    }
    Ok(())
}

fn load_solver_pruning(
    tables: &TransitionTables,
    out_dir: &Path,
    progress_interval: usize,
) -> Result<PatternDatabases, String> {
    fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
    let specs = [
        parse_candidate("edge3+uf3")?,
        parse_candidate("corner+uf3")?,
    ];
    let mut loaded = Vec::new();
    for spec in specs {
        let name = spec.name();
        let path = out_dir.join(format!("{}.pdb", file_safe_name(&name)));
        eprintln!(
            "loading pruning table {} ({:.1} MiB)",
            name,
            spec.size().unwrap_or(0) as f64 / (1024.0 * 1024.0)
        );
        loaded.push(PatternDatabase::load_or_build(
            spec,
            tables,
            usize::MAX,
            progress_interval,
            path,
        )?);
    }
    let pruning = PatternDatabases::new(loaded);
    eprintln!(
        "using {} pruning tables: {} ({:.1} MiB)",
        pruning.len(),
        pruning.names().join(" / "),
        pruning.total_bytes() as f64 / (1024.0 * 1024.0)
    );
    Ok(pruning)
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
    Ok(())
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
                let candidate = fto_core::moves::Move::ALL[rng.next_usize(10)];
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

If no state is provided, the solved state is used."
    );
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
    use fto_core::moves::Move;

    match token {
        "U" => Ok(Move::U),
        "U'" | "Ui" => Ok(Move::Up),
        "B" => Ok(Move::B),
        "B'" | "Bi" => Ok(Move::Bp),
        "R" => Ok(Move::R),
        "R'" | "Ri" => Ok(Move::Rp),
        "L" => Ok(Move::L),
        "L'" | "Li" => Ok(Move::Lp),
        "Rw" => Ok(Move::Rw),
        "Rw'" | "Rwi" => Ok(Move::Rwp),
        _ => Err(format!("unknown move: {token}")),
    }
}
