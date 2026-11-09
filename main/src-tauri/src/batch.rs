//! Depth-major batch scheduling. Completed depths are the resume checkpoint;
//! interrupted depths retain portable DFS stacks alongside streamed solutions.
use super::*;
use std::collections::BTreeSet;

thread_local! { pub(super) static BATCH_LOG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[derive(Default)]
pub(super) struct BatchState {
    active: AtomicBool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Solution {
    depth: u8,
    algorithm: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Case {
    id: String,
    setup: String,
    facelets: Option<Vec<u8>>,
    center_targets: Option<CenterTargets>,
    #[serde(default)]
    completed_depths: BTreeSet<u8>,
    first_depth: Option<u8>,
    #[serde(default)]
    deferred_depth: Option<u8>,
    #[serde(default)]
    checkpoint: Option<search::ResumeCheckpoint>,
    #[serde(default)]
    solutions: Vec<Solution>,
    #[serde(default)]
    nodes: u64,
    error: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Request {
    run_id: String,
    cases: Vec<Case>,
    allowed_moves: Vec<String>,
    instance_moves: Vec<String>,
    depths: Option<Vec<u8>>,
    find_all: bool,
    last_layer_mode: bool,
    threads: usize,
    #[serde(default = "default_slow_case_seconds")]
    slow_case_seconds: f64,
    #[serde(default = "default_use_gpu")]
    use_gpu: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    run_id: String,
    kind: String,
    case_id: Option<String>,
    depth: Option<u8>,
    algorithm: Option<String>,
    case: Option<Case>,
    message: Option<String>,
}
fn event(
    app: &AppHandle,
    request: &Request,
    kind: &str,
    case: Option<Case>,
    depth: Option<u8>,
    message: Option<String>,
) {
    let _ = app.emit(
        "batch-event",
        Event {
            run_id: request.run_id.clone(),
            kind: kind.into(),
            case_id: case.as_ref().map(|c| c.id.clone()),
            case,
            depth,
            algorithm: None,
            message,
        },
    );
}

fn default_slow_case_seconds() -> f64 {
    10.0
}

#[derive(Debug, PartialEq)]
enum WaveKind {
    Coverage,
    CatchUp,
    Deferred,
    Expansion,
}
struct Wave {
    depth: u8,
    indices: Vec<usize>,
    kind: WaveKind,
}

// A timed-out depth remains incomplete. Bring everyone else up to the earliest
// deferred depth before retrying its held cases, without a time limit this time.
fn plan_wave(cases: &[Case], depths: &[u8], expand: bool) -> Option<Wave> {
    let barrier = cases
        .iter()
        .filter(|c| c.error.is_none())
        .filter_map(|c| {
            c.deferred_depth
                .filter(|d| depths.contains(d) && !c.completed_depths.contains(d))
        })
        .min();
    if let Some(barrier) = barrier {
        for &depth in depths.iter().filter(|&&d| d <= barrier) {
            let indices: Vec<_> = cases
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    c.error.is_none()
                        && c.deferred_depth.is_none()
                        && !c.completed_depths.contains(&depth)
                })
                .map(|(i, _)| i)
                .collect();
            if !indices.is_empty() {
                return Some(Wave {
                    depth,
                    indices,
                    kind: WaveKind::CatchUp,
                });
            }
        }
        let indices = cases
            .iter()
            .enumerate()
            .filter(|(_, c)| c.error.is_none() && c.deferred_depth == Some(barrier))
            .map(|(i, _)| i)
            .collect();
        return Some(Wave {
            depth: barrier,
            indices,
            kind: WaveKind::Deferred,
        });
    }
    let coverage = cases.iter().any(|c| {
        c.error.is_none()
            && c.first_depth.is_none()
            && depths.iter().any(|d| !c.completed_depths.contains(d))
    });
    if !coverage && !expand {
        return None;
    }
    for &depth in depths {
        let indices: Vec<_> = cases
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.error.is_none()
                    && (!coverage || c.first_depth.is_none())
                    && !c.completed_depths.contains(&depth)
            })
            .map(|(i, _)| i)
            .collect();
        if !indices.is_empty() {
            return Some(Wave {
                depth,
                indices,
                kind: if coverage {
                    WaveKind::Coverage
                } else {
                    WaveKind::Expansion
                },
            });
        }
    }
    None
}
#[cfg(test)]
fn next_wave(cases: &[Case], depths: &[u8], expand: bool) -> Option<(u8, Vec<usize>)> {
    plan_wave(cases, depths, expand).map(|wave| (wave.depth, wave.indices))
}

// Cancellation belongs to an individual attempt. The monitor also forwards a
// project pause to it. A finished attempt wakes its monitor immediately.
fn timed_attempt<T>(
    global: &Arc<AtomicBool>,
    budget: Option<std::time::Duration>,
    action: impl FnOnce(Arc<AtomicBool>) -> T,
) -> (T, bool, bool) {
    if budget.is_none() {
        let result = action(global.clone());
        return (result, false, global.load(Ordering::SeqCst));
    }
    let local = Arc::new(AtomicBool::new(global.load(Ordering::SeqCst)));
    let timed_out = AtomicBool::new(false);
    let finished = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    struct Finish(Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl Drop for Finish {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = true;
            self.0 .1.notify_all();
        }
    }
    let result = std::thread::scope(|scope| {
        let local = &local;
        let timed_out = &timed_out;
        let finished = &finished;
        scope.spawn(move || {
            let start = Instant::now();
            let mut done = finished.0.lock().unwrap();
            loop {
                if *done {
                    break;
                }
                if global.load(Ordering::SeqCst) {
                    local.store(true, Ordering::SeqCst);
                    break;
                }
                if budget.is_some_and(|limit| start.elapsed() >= limit) {
                    timed_out.store(true, Ordering::SeqCst);
                    local.store(true, Ordering::SeqCst);
                    break;
                }
                let wait = budget.map_or(std::time::Duration::from_millis(25), |limit| {
                    limit
                        .saturating_sub(start.elapsed())
                        .min(std::time::Duration::from_millis(25))
                });
                done = finished.1.wait_timeout(done, wait).unwrap().0;
            }
        });
        let guard = Finish(finished.clone());
        let result = action(local.clone());
        drop(guard);
        result
    });
    (
        result,
        timed_out.load(Ordering::SeqCst),
        local.load(Ordering::SeqCst),
    )
}

fn allocation(threads: usize, count: usize) -> Vec<usize> {
    let jobs = count.min(threads.max(1));
    (0..jobs)
        .map(|i| threads.max(1) / jobs + usize::from(i < threads.max(1) % jobs))
        .collect()
}

// Refill available slots immediately instead of waiting for the slowest case in
// a chunk. A depth barrier remains in place between calls to this function.
fn execute_wave<T: Send>(
    indices: &[usize],
    threads: usize,
    cancel: &AtomicBool,
    action: impl Fn(usize, usize) -> T + Sync,
) -> Result<Vec<(usize, T)>, String> {
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut pending: std::collections::VecDeque<_> = indices.iter().copied().collect();
        let mut available = threads.max(1);
        let mut active = 0;
        let mut results = Vec::new();
        let mut failed = false;
        loop {
            if cancel.load(Ordering::SeqCst) {
                pending.clear();
            }
            if available > 0 && !pending.is_empty() {
                for budget in allocation(available, pending.len()) {
                    let index = pending.pop_front().unwrap();
                    available -= budget;
                    active += 1;
                    let tx = tx.clone();
                    let action = &action;
                    scope.spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            action(index, budget)
                        }));
                        let _ = tx.send((index, budget, result));
                    });
                }
            }
            if active == 0 {
                break;
            }
            let (index, budget, result) = rx.recv().map_err(|_| "Batch worker disconnected.")?;
            available += budget;
            active -= 1;
            match result {
                Ok(value) => results.push((index, value)),
                Err(_) => {
                    failed = true;
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        }
        if failed {
            Err("Batch worker failed.".into())
        } else {
            Ok(results)
        }
    })
}

#[tauri::command]
pub(super) async fn export_batch_file(
    app: AppHandle,
    name: String,
    kind: String,
    content: String,
) -> Result<String, String> {
    let suffix = match kind.as_str() {
        "csv" => "csv",
        "project" => "ftobatch.json",
        _ => return Err("Unknown export format.".into()),
    };
    let directory = app.path().download_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || write_export(&directory, &name, suffix, &content))
        .await
        .map_err(|e| e.to_string())?
}
fn write_export(
    directory: &Path,
    name: &str,
    suffix: &str,
    content: &str,
) -> Result<String, String> {
    use std::io::Write;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let stem: String = name
        .chars()
        .take(100)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = if stem.trim().is_empty() {
        "FTO batch"
    } else {
        stem.trim()
    };
    for index in 0..10000 {
        let filename = if index == 0 {
            format!("{stem}.{suffix}")
        } else {
            format!("{stem} ({index}).{suffix}")
        };
        let path = directory.join(filename);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(e) = file
                    .write_all(content.as_bytes())
                    .and_then(|_| file.sync_all())
                {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(e.to_string());
                }
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many exports with this name. Rename the project and try again.".into())
}

#[tauri::command]
pub(super) fn batch_status(state: tauri::State<'_, BatchState>) -> bool {
    state.active.load(Ordering::SeqCst)
}

#[tauri::command]
pub(super) async fn pause_batch(
    state: tauri::State<'_, BatchState>,
    solver: tauri::State<'_, SolverState>,
) -> Result<(), String> {
    if state.active.load(Ordering::SeqCst) {
        if let Some(cancel) = solver
            .current_cancel
            .lock()
            .map_err(|_| "solver lock poisoned")?
            .as_ref()
        {
            cancel.store(true, Ordering::SeqCst);
        }
    }
    Ok(())
}

#[tauri::command]
pub(super) async fn start_batch(
    request: Request,
    solver: tauri::State<'_, SolverState>,
    state: tauri::State<'_, BatchState>,
    app: AppHandle,
) -> Result<(), String> {
    if request.cases.is_empty() {
        return Err("Add at least one case.".into());
    }
    if request.threads == 0 || request.threads > 1024 {
        return Err("Threads must be between 1 and 1024.".into());
    }
    if parse_move_names(&request.allowed_moves)?.is_empty() {
        return Err("Select at least one move.".into());
    }
    depths::normalize(request.depths.as_deref(), None)?;
    if !request.slow_case_seconds.is_finite()
        || !(0.0..=86400.0).contains(&request.slow_case_seconds)
    {
        return Err("Slow-case budget must be between 0 and 86400 seconds.".into());
    }
    let ids: HashSet<_> = request.cases.iter().map(|c| &c.id).collect();
    if ids.len() != request.cases.len() {
        return Err("Case identifiers must be unique.".into());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut current = solver
            .current_cancel
            .lock()
            .map_err(|_| "solver lock poisoned")?;
        if current.is_some() {
            return Err("Another search or table generation is running.".into());
        }
        *current = Some(cancel.clone());
        state.active.store(true, Ordering::SeqCst);
    }
    let cache = solver.cache.clone();
    let current = solver.current_cancel.clone();
    tauri::async_runtime::spawn(async move {
        let run_app = app.clone();
        let run_request = request.clone();
        let run_cancel = cancel.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            struct LogScope(bool);
            impl Drop for LogScope {
                fn drop(&mut self) {
                    BATCH_LOG.with(|flag| flag.set(self.0));
                }
            }
            let _scope = LogScope(BATCH_LOG.with(|flag| flag.replace(true)));
            with_gpu_generation(run_request.use_gpu, &run_app, || {
                run(&run_request, &run_cancel, &run_app, &cache)
            })
        })
        .await;
        // Release the shared solver before announcing that resume is available.
        if let Ok(mut current) = current.lock() {
            *current = None;
        }
        app.state::<BatchState>()
            .active
            .store(false, Ordering::SeqCst);
        match result {
            Ok(Ok(())) => event(
                &app,
                &request,
                if cancel.load(Ordering::SeqCst) {
                    "paused"
                } else {
                    "done"
                },
                None,
                None,
                None,
            ),
            Ok(Err(message)) => event(
                &app,
                &request,
                if cancel.load(Ordering::SeqCst) {
                    "paused"
                } else {
                    "error"
                },
                None,
                None,
                Some(message),
            ),
            Err(error) => event(&app, &request, "error", None, None, Some(error.to_string())),
        }
    });
    Ok(())
}

fn run(
    request: &Request,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
    cache: &Mutex<SolverCache>,
) -> Result<(), String> {
    let allowed = parse_move_names(&request.allowed_moves)?;
    let instance = if request.instance_moves.is_empty() {
        Move::ALL.to_vec()
    } else {
        parse_move_names(&request.instance_moves)?
    };
    if !allowed.iter().all(|m| instance.contains(m)) {
        return Err("Selected moves must belong to the instance.".into());
    }
    let depths =
        depths::normalize(request.depths.as_deref(), None)?.unwrap_or_else(|| (0..=255).collect());
    let expand = request.find_all && request.depths.is_some();
    let mut cases = request.cases.clone();
    let mut positions = Vec::with_capacity(cases.len());
    for case in &mut cases {
        let parsed = if let Some(facelets) = &case.facelets {
            cubie_and_partial_mask_from_facelets(
                facelets,
                case.center_targets.as_ref(),
                request.last_layer_mode,
            )
        } else {
            apply_sequence(FtoCubie::solved(), &case.setup).map(|c| (c, None))
        };
        match parsed {
            Ok(position) => {
                case.error = None;
                positions.push(Some(position));
            }
            Err(error) => {
                case.error = Some(error);
                positions.push(None);
                event(app, request, "case", Some(case.clone()), None, None);
            }
        }
    }
    if plan_wave(&cases, &depths, expand).is_none() {
        return Ok(());
    }
    event(
        app,
        request,
        "status",
        None,
        None,
        Some("Preparing shared search tables…".into()),
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let tables = load_transition_tables(
        cache,
        &transition_dir(app, root).join("transition-table.tran"),
        app,
    )?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let pruning_path = pruning_dir(app, root);
    // One shared set of pattern databases regardless of the number of cases.
    let needs_full = positions.iter().flatten().any(|(_, mask)| mask.is_none());
    let pruning = if needs_full && !request.last_layer_mode {
        Some(load_solver_pruning(
            cache,
            &tables,
            &pruning_path,
            pruning_progress_interval(allowed.len()),
            &allowed,
            &instance,
            false,
            request.threads,
            cancel,
            app,
        )?)
    } else {
        None
    };
    let ll_pruning = if needs_full && request.last_layer_mode {
        Some(load_ll_solver_pruning(
            cache,
            &tables,
            &pruning_path,
            pruning_progress_interval(allowed.len()),
            &allowed,
            &instance,
            false,
            request.threads,
            cancel,
            app,
        )?)
    } else {
        None
    };
    while !cancel.load(Ordering::SeqCst) {
        let Some(wave) = plan_wave(&cases, &depths, expand) else {
            break;
        };
        let depth = wave.depth;
        let indices = wave.indices;
        let phase = match wave.kind {
            WaveKind::Coverage => "First solutions",
            WaveKind::CatchUp => "Catching up",
            WaveKind::Deferred => "Deferred cases",
            WaveKind::Expansion => "Remaining depths",
        };
        let budget = if request.find_all
            && request.slow_case_seconds > 0.0
            && matches!(wave.kind, WaveKind::Coverage | WaveKind::CatchUp)
        {
            Some(std::time::Duration::from_secs_f64(
                request.slow_case_seconds,
            ))
        } else {
            None
        };
        event(
            app,
            request,
            "status",
            None,
            Some(depth),
            Some(format!("{phase} · depth {depth}")),
        );
        let bloom = if needs_full {
            bloom_cache::load(
                cache,
                &pruning_path,
                &allowed,
                Some(depth),
                request.last_layer_mode,
                cancel,
                app,
            )?
        } else {
            None
        };
        let results = execute_wave(&indices, request.threads, cancel, |index, threads| {
            let case = cases[index].clone();
            let position = positions[index].clone().unwrap();
            let tables = &tables;
            event(
                app,
                request,
                "search",
                Some(case.clone()),
                Some(depth),
                None,
            );
            // Resumed depths retain existing results and emit only new algorithms.
            let found = Arc::new(Mutex::new((
                Vec::<String>::new(),
                case.solutions
                    .iter()
                    .filter(|s| s.depth == depth)
                    .map(|s| s.algorithm.clone())
                    .collect::<HashSet<_>>(),
            )));
            let reported = found.clone();
            let report_app = app.clone();
            let run_id = request.run_id.clone();
            let case_id = case.id.clone();
            let find_all = request.find_all;
            let reporter: Arc<dyn Fn(&[Move]) + Send + Sync> = Arc::new(move |moves| {
                let algorithm = search::format_solution(moves);
                let mut found = reported.lock().unwrap();
                if (!find_all && !found.1.is_empty()) || !found.1.insert(algorithm.clone()) {
                    return;
                }
                found.0.push(algorithm.clone());
                let _ = report_app.emit(
                    "batch-event",
                    Event {
                        run_id: run_id.clone(),
                        kind: "solution".into(),
                        case_id: Some(case_id.clone()),
                        depth: Some(depth),
                        algorithm: Some(algorithm),
                        case: None,
                        message: None,
                    },
                );
            });
            let (result, timed_out, _interrupted) =
                timed_attempt(cancel, budget, |attempt_cancel| {
                    let config = SearchConfig {
                        min_depth: depth,
                        max_depth: depth,
                        find_all,
                        allowed_moves: allowed.clone(),
                        free_u_ends: request.last_layer_mode,
                        cancel: Some(attempt_cancel),
                        bloom: if position.1.is_none() {
                            bloom.clone()
                        } else {
                            None
                        },
                        solution_reporter: Some(reporter),
                    };
                    resume_position(
                        position,
                        tables,
                        pruning.as_deref(),
                        ll_pruning.as_deref(),
                        &config,
                        threads,
                        case.checkpoint.as_ref(),
                    )
                });
            let mut case = case;
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    case.error = Some(error);
                    event(app, request, "case", Some(case.clone()), Some(depth), None);
                    return case;
                }
            };
            case.checkpoint = result.checkpoint;
            case.nodes = case.nodes.saturating_add(result.result.nodes);
            for algorithm in &found.lock().unwrap().0 {
                case.solutions.push(Solution {
                    depth,
                    algorithm: algorithm.clone(),
                });
            }
            if timed_out && case.checkpoint.is_some() && !cancel.load(Ordering::SeqCst) {
                case.deferred_depth = Some(depth);
            }
            if case.checkpoint.is_none() {
                case.deferred_depth = None;
                case.completed_depths.insert(depth);
                if case.solutions.iter().any(|s| s.depth == depth) {
                    case.first_depth = Some(case.first_depth.map_or(depth, |d| d.min(depth)));
                }
            }
            event(app, request, "case", Some(case.clone()), Some(depth), None);
            case
        })?;
        for (index, case) in results {
            cases[index] = case;
        }
    }
    Ok(())
}

fn resume_position(
    position: (FtoCubie, Option<PartialMask>),
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    ll_pruning: Option<&PatternDatabases>,
    config: &SearchConfig,
    threads: usize,
    checkpoint: Option<&search::ResumeCheckpoint>,
) -> Result<search::ResumeResult, String> {
    if let Some(mask) = position.1 {
        let problem = PartialProblem {
            cubie: position.0,
            mask,
        };
        partial::PreparedPartialSearch::new(&problem, config, None)
            .resume_depth(config, threads, checkpoint)
    } else {
        search::resume_exact(
            position.0, tables, pruning, ll_pruning, config, threads, checkpoint,
        )
    }
}

#[cfg(test)]
fn search_position(
    position: (FtoCubie, Option<PartialMask>),
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    ll_pruning: Option<&PatternDatabases>,
    config: &SearchConfig,
    threads: usize,
) -> search::SearchResult {
    resume_position(position, tables, pruning, ll_pruning, config, threads, None)
        .unwrap()
        .result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn case(id: &str) -> Case {
        Case {
            id: id.into(),
            setup: "".into(),
            facelets: None,
            center_targets: None,
            completed_depths: BTreeSet::new(),
            first_depth: None,
            deferred_depth: None,
            checkpoint: None,
            solutions: vec![],
            nodes: 0,
            error: None,
        }
    }
    #[test]
    fn depth_major_coverage_then_expansion_and_resume() {
        let mut cases = vec![case("a"), case("b")];
        let depths = [1, 2, 4];
        assert_eq!(next_wave(&cases, &depths, true), Some((1, vec![0, 1])));
        for c in &mut cases {
            c.completed_depths.insert(1);
        }
        cases[0].first_depth = Some(1);
        assert_eq!(next_wave(&cases, &depths, true), Some((2, vec![1])));
        cases[1].solutions.push(Solution {
            depth: 2,
            algorithm: "U".into(),
        });
        // Streaming a solution does not skip its unfinished depth on resume.
        assert_eq!(next_wave(&cases, &depths, true), Some((2, vec![1])));
        cases[1].completed_depths.insert(2);
        cases[1].first_depth = Some(2);
        assert_eq!(next_wave(&cases, &depths, false), None);
        assert_eq!(next_wave(&cases, &depths, true), Some((2, vec![0])));
        cases[0].completed_depths.insert(2);
        assert_eq!(next_wave(&cases, &depths, true), Some((4, vec![0, 1])));
        for c in &mut cases {
            c.completed_depths.insert(4);
        }
        assert_eq!(next_wave(&cases, &depths, true), None);
        cases.push(case("new"));
        assert_eq!(next_wave(&cases, &depths, true), Some((1, vec![2])));
        cases.remove(2);
        assert_eq!(next_wave(&cases, &depths, true), None);
    }
    #[test]
    fn exhausted_and_invalid_cases_do_not_block_other_cases() {
        let mut cases = vec![case("a"), case("b"), case("bad")];
        cases[0].first_depth = Some(1);
        cases[0].completed_depths.insert(1);
        cases[1].completed_depths.extend([1, 2]);
        cases[2].error = Some("bad setup".into());
        assert_eq!(next_wave(&cases, &[1, 2], true), Some((2, vec![0])));
    }
    #[test]
    fn allocates_cases_before_threads_without_oversubscription() {
        assert_eq!(allocation(8, 10), vec![1; 8]);
        assert_eq!(allocation(8, 3), vec![3, 3, 2]);
        assert_eq!(allocation(8, 1), vec![8]);
    }
    #[test]
    #[ignore = "integration check uses the locally generated transition table"]
    fn real_search_covers_cases_then_finishes_selected_depths() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let tables = TransitionTables::load(root.join("cache/transition-table.tran")).unwrap();
        let mut cases = vec![case("one"), case("two"), case("solved")];
        let positions: Vec<_> = ["R", "R U", ""]
            .iter()
            .map(|s| apply_sequence(FtoCubie::solved(), s).unwrap())
            .collect();
        let mut waves = vec![];
        while let Some((depth, indices)) = next_wave(&cases, &[0, 1, 2, 3], true) {
            waves.push((depth, indices.clone()));
            for &index in &indices {
                let config = SearchConfig {
                    min_depth: depth,
                    max_depth: depth,
                    allowed_moves: vec![Move::R, Move::Rp, Move::U, Move::Up],
                    find_all: true,
                    ..SearchConfig::default()
                };
                let streamed = Arc::new(Mutex::new(Vec::new()));
                let report = streamed.clone();
                let config = SearchConfig {
                    solution_reporter: Some(Arc::new(move |moves| {
                        report.lock().unwrap().push(moves.to_vec())
                    })),
                    ..config
                };
                let result =
                    search_position((positions[index], None), &tables, None, None, &config, 4);
                let mut reported = streamed.lock().unwrap().clone();
                let mut returned = result.solutions.clone();
                reported.sort_by_key(|moves| moves.iter().map(|m| m.idx()).collect::<Vec<_>>());
                returned.sort_by_key(|moves| moves.iter().map(|m| m.idx()).collect::<Vec<_>>());
                assert_eq!(
                    reported, returned,
                    "streamed and returned results at depth {depth}"
                );
                let case = &mut cases[index];
                case.completed_depths.insert(depth);
                for solution in result.solutions {
                    let mut end = positions[index];
                    for mv in &solution {
                        end = end.apply(*mv);
                    }
                    assert_eq!(end.coord(), FtoCubie::solved().coord());
                    case.first_depth = Some(case.first_depth.map_or(depth, |d| d.min(depth)));
                    case.solutions.push(Solution {
                        depth,
                        algorithm: search::format_solution(&solution),
                    });
                }
            }
        }
        assert_eq!(
            &waves[..3],
            &[(0, vec![0, 1, 2]), (1, vec![0, 1]), (2, vec![1])]
        );
        assert_eq!(waves[3], (1, vec![2])); // expansion starts only after coverage
        assert_eq!(
            cases.iter().map(|c| c.first_depth).collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(0)]
        );
        assert!(cases.iter().all(|c| c.completed_depths.len() == 4));
        let config = SearchConfig {
            min_depth: 0,
            max_depth: 0,
            free_u_ends: true,
            allowed_moves: vec![Move::R, Move::Rp],
            ..SearchConfig::default()
        };
        let ll = search_position(
            (FtoCubie::solved().apply(Move::U), None),
            &tables,
            None,
            None,
            &config,
            2,
        );
        assert!(
            !ll.solutions.is_empty(),
            "last-layer mode retains free U ends"
        );
    }
    #[test]
    fn fast_case_refills_queue_while_another_case_is_still_busy() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let release_rx = Mutex::new(release_rx);
            execute_wave(&[0, 1, 2], 2, &AtomicBool::new(false), |index, budget| {
                assert_eq!(budget, 1);
                if index == 0 {
                    release_rx.lock().unwrap().recv().unwrap();
                }
                if index == 2 {
                    entered_tx.send(()).unwrap();
                }
                index
            })
            .unwrap()
        });
        let entered = entered_rx.recv_timeout(std::time::Duration::from_secs(3));
        release_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap().len(), 3);
        entered.expect("third case should start before the slow first case finishes");
    }
    #[test]
    fn exports_preserve_utf8_and_never_overwrite_or_escape_download_directory() {
        let directory =
            std::env::temp_dir().join(format!("fto-batch-export-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let first = write_export(
            &directory,
            "../../batch",
            "csv",
            "\u{feff}Case,Algorithm\r\n1,U′\r\n",
        )
        .unwrap();
        let second = write_export(&directory, "../../batch", "csv", "second").unwrap();
        assert_ne!(first, second);
        assert_eq!(Path::new(&first).parent(), Some(directory.as_path()));
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "\u{feff}Case,Algorithm\r\n1,U′\r\n"
        );
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn deferred_case_runs_after_parked_cases_catch_up_without_skipping_depths() {
        let mut cases = vec![case("a"), case("b"), case("slow")];
        cases[0].first_depth = Some(1);
        cases[0].completed_depths.extend([0, 1]);
        cases[1].first_depth = Some(2);
        cases[1].completed_depths.extend([0, 1, 2]);
        cases[2].completed_depths.extend([0, 1, 2]);
        cases[2].deferred_depth = Some(3);
        let depths = [0, 1, 2, 3, 4];
        let wave = plan_wave(&cases, &depths, true).unwrap();
        assert_eq!(
            (wave.depth, wave.indices, wave.kind),
            (2, vec![0], WaveKind::CatchUp)
        );
        cases[0].completed_depths.insert(2);
        let wave = plan_wave(&cases, &depths, true).unwrap();
        assert_eq!(
            (wave.depth, wave.indices, wave.kind),
            (3, vec![0, 1], WaveKind::CatchUp)
        );
        cases[0].completed_depths.insert(3);
        cases[1].completed_depths.insert(3);
        let wave = plan_wave(&cases, &depths, true).unwrap();
        assert_eq!(
            (wave.depth, wave.indices, wave.kind),
            (3, vec![2], WaveKind::Deferred)
        );
        assert!(!cases[2].completed_depths.contains(&3));
        // A retry with no solution completes its held depth and resumes coverage.
        cases[2].deferred_depth = None;
        cases[2].completed_depths.insert(3);
        let wave = plan_wave(&cases, &depths, true).unwrap();
        assert_eq!(
            (wave.depth, wave.indices, wave.kind),
            (4, vec![2], WaveKind::Coverage)
        );
    }
    #[test]
    fn earliest_deferred_depth_wins_and_new_cases_catch_up_on_resume() {
        let mut cases = vec![case("slow2"), case("slow3"), case("new")];
        cases[0].completed_depths.extend([0, 1]);
        cases[0].deferred_depth = Some(2);
        cases[1].completed_depths.extend([0, 1, 2]);
        cases[1].deferred_depth = Some(3);
        let depths = [0, 1, 2, 3];
        for expected in 0..=2 {
            let wave = plan_wave(&cases, &depths, true).unwrap();
            assert_eq!(
                (wave.depth, wave.indices, wave.kind),
                (expected, vec![2], WaveKind::CatchUp)
            );
            cases[2].completed_depths.insert(expected);
            cases[2].first_depth = Some(0);
        }
        let wave = plan_wave(&cases, &depths, true).unwrap();
        assert_eq!(
            (wave.depth, wave.indices, wave.kind),
            (2, vec![0], WaveKind::Deferred)
        );
        cases[0].completed_depths.insert(2);
        cases[0].deferred_depth = None;
        let wave = plan_wave(&cases, &depths, true).unwrap();
        assert_eq!(
            (wave.depth, wave.indices, wave.kind),
            (3, vec![0, 2], WaveKind::CatchUp)
        );
    }
    #[test]
    fn time_budget_cancels_only_one_case_and_pause_still_cancels_attempts() {
        let global = Arc::new(AtomicBool::new(false));
        let (_, timed_out, interrupted) = timed_attempt(
            &global,
            Some(std::time::Duration::from_millis(20)),
            |local| {
                while !local.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            },
        );
        assert!(timed_out && interrupted);
        assert!(!global.load(Ordering::SeqCst));
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                global.store(true, Ordering::SeqCst);
            });
            let (_, timed_out, interrupted) = timed_attempt(&global, None, |local| {
                while !local.load(Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            });
            assert!(!timed_out && interrupted);
        });
        let (result, timed_out, interrupted) = timed_attempt(
            &Arc::new(AtomicBool::new(false)),
            Some(std::time::Duration::from_secs(10)),
            |_| 42,
        );
        assert_eq!(result, 42);
        assert!(!timed_out && !interrupted);
    }
}
