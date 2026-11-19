use super::*;
use fto_core::bloom::{self, BloomMode, BloomTable, GenerateConfig};

pub(super) struct CachedBloom {
    pub path: PathBuf,
    pub table: Arc<BloomTable>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateBloomRequest {
    #[serde(default = "default_use_gpu")]
    use_gpu: bool,
    moves: Vec<String>,
    depth: u8,
    #[serde(default)]
    last_layer_mode: bool,
    max_memory_mib: u32,
    #[serde(default)]
    facelets: Option<Vec<u8>>,
    #[serde(default)]
    center_targets: Option<CenterTargets>,
    #[serde(default = "default_partial_budget")]
    partial_memory_mib: u32,
}

#[cfg_attr(feature = "desktop", tauri::command)]
pub(crate) async fn generate_bloom_table(
    request: GenerateBloomRequest,
    solver_state: tauri::State<'_, SolverState>,
    app: AppHandle,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut current = solver_state
            .current_cancel
            .lock()
            .map_err(|_| "solver state lock poisoned")?;
        if current.is_some() {
            return Err("a solve or table build is already running".into());
        }
        *current = Some(cancel.clone());
        solver_state
            .operation_log
            .lock()
            .map_err(|_| "operation log lock poisoned")?
            .clear();
    }
    let cancels = solver_state.current_cancel.clone();
    let cache = solver_state.cache.clone();
    let generation = solver_state.generation.clone();
    let _job = tauri::async_runtime::spawn_blocking(move || {
        let result = with_gpu_generation(request.use_gpu, &app, &generation, || {
            generate(&request, &cancel, &app, &cache)
        });
        if let Ok(mut current) = cancels.lock() {
            *current = None;
        }
        if cancel.load(Ordering::Relaxed) {
            emit_line(&app, "cancel", "Bloom table build cancelled");
            let _ = app.emit("solve-cancelled", ());
        } else {
            match result {
                Ok(()) => {
                    let _ = app.emit("bloom-generated", ());
                }
                Err(error) => {
                    emit_line(&app, "error", &format!("error: {error}"));
                    let _ = app.emit("solve-error", error);
                }
            }
        }
    });
    Ok(())
}
fn generate(
    request: &GenerateBloomRequest,
    cancel: &Arc<AtomicBool>,
    app: &AppHandle,
    cache: &Mutex<SolverCache>,
) -> Result<(), String> {
    let moves = parse_move_names(&request.moves)?;
    if moves.is_empty() {
        return Err("select at least one Bloom move".into());
    }
    // Validate before loading or generating the large shared transition tables.
    if !(1..=16).contains(&request.depth) || request.max_memory_mib == 0 {
        return Err("Bloom depth must be 1–16 and memory budget must be at least 1 MiB".into());
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let tables = load_transition_tables(
        cache,
        &transition_dir(app, workspace).join("transition-table.tran"),
        app,
        cancel,
    )?;
    if let Some(facelets) = &request.facelets {
        if !(1..=4096).contains(&request.partial_memory_mib) {
            return Err("Partial projection budget must be between 1 and 4096 MiB".into());
        }
        let (_, mask) = cubie_and_partial_mask_from_facelets(
            facelets,
            request.center_targets.as_ref(),
            request.last_layer_mode,
        )?;
        let mask = mask.ok_or("Undefine at least one piece for a partial Bloom table.")?;
        let plan = fto_core::partial_goal::plan(
            &mask,
            &tables,
            partial_memory_budget(request.partial_memory_mib),
            Some(cancel),
        )?;
        let meta = fto_core::partial_goal::metadata(&plan, &mask, &moves, Some(request.depth));
        let progress =
            |p: pruning::PruningProgress| emit_line(app, "progress", &format_pruning_progress(&p));
        emit_line(
            app,
            "info",
            &format!(
                "generating partial Bloom projection {} ({} workspace)",
                plan.spec.name(),
                format_bytes(plan.entries as u64)
            ),
        );
        fto_core::partial_goal::generate(
            &plan,
            &meta,
            &tables,
            &pruning_dir(app, workspace).join("partial-goal-v1"),
            std::thread::available_parallelism().map_or(1, usize::from),
            Some(cancel),
            Some(&progress),
            request.max_memory_mib,
        )?;
        emit_line(app, "done", "generated dedicated partial Bloom table");
        return Ok(());
    }
    emit_line(
        app,
        "info",
        &format!(
            "generating {}depth-{} Bloom table for {} (memory cap {})",
            if request.last_layer_mode {
                "last-layer "
            } else {
                ""
            },
            request.depth,
            format_move_list(&moves),
            format_bytes(u64::from(request.max_memory_mib) * 1024 * 1024)
        ),
    );
    let progress = |event: &bloom::BloomProgress| {
        let percent = 100. * event.visited as f64 / event.total.max(1) as f64;
        emit_line(
            app,
            "progress",
            &format!(
                "bloom progress: depth {} paths {}/{} ({percent:.1}%)",
                request.depth, event.visited, event.total
            ),
        );
    };
    let mode = if request.last_layer_mode {
        BloomMode::LastLayer
    } else {
        BloomMode::FullPosition
    };
    {
        let mut locked = cache.lock().map_err(|_| "solver cache lock poisoned")?;
        locked.bloom = None;
        locked.secondary_bloom = None;
    }
    let table = BloomTable::generate(
        &tables,
        &moves,
        GenerateConfig {
            mode,
            depth: request.depth,
            max_memory_mib: request.max_memory_mib,
        },
        Some(cancel),
        Some(&progress),
    )?;
    emit_line(app, "info", "saving completed Bloom table...");
    let path = pruning_dir(app, workspace)
        .join("bloom-v1")
        .join(bloom::cache_file_name(&moves, request.depth, mode));
    table.save(&path, Some(cancel))?;
    let info = table.info();
    cache
        .lock()
        .map_err(|_| "solver cache lock poisoned")?
        .bloom = Some(CachedBloom {
        path,
        table: Arc::new(table),
    });
    emit_line(
        app,
        "done",
        &format!(
            "generated {}depth-{} Bloom table ({}, estimated false positives {:.3}%)",
            if request.last_layer_mode {
                "last-layer "
            } else {
                ""
            },
            info.depth,
            format_bytes(info.bytes),
            info.false_positive_rate * 100.
        ),
    );
    Ok(())
}
pub(super) fn discover(root: &Path) -> Result<Vec<PruningTableInfo>, String> {
    let mut result = Vec::new();
    let entries = match fs::read_dir(root.join("bloom-v1")) {
        Ok(v) => v,
        Err(_) => return Ok(result),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".bloom") {
            continue;
        }
        let Ok(info) = BloomTable::read_info(&entry.path()) else {
            continue;
        };
        if name != bloom::cache_file_name(&info.moves, info.depth, info.mode) {
            continue;
        }
        result.push(PruningTableInfo {
            id: format!("bloom-{}", name.trim_end_matches(".bloom")),
            codename: format!(
                "{}Bloom depth {}",
                if info.mode == BloomMode::LastLayer {
                    "Last-layer "
                } else {
                    ""
                },
                info.depth
            ),
            last_layer_mode: info.mode == BloomMode::LastLayer,
            kind: "bloom".into(),
            depth: Some(info.depth),
            moves: info.moves.iter().map(|m| m.name().to_owned()).collect(),
            files: vec![format!("bloom-v1/{name}")],
            bytes: info.bytes,
        });
    }
    Ok(result)
}
pub(super) fn load(
    cache: &Mutex<SolverCache>,
    root: &Path,
    moves: &[Move],
    max_depth: Option<u8>,
    last_layer_mode: bool,
    cancel: &AtomicBool,
    app: &AppHandle,
) -> Result<Option<Arc<BloomTable>>, String> {
    load_from_roots(
        cache,
        &[root],
        moves,
        max_depth,
        last_layer_mode,
        cancel,
        app,
    )
}

pub(super) fn load_from_roots(
    cache: &Mutex<SolverCache>,
    roots: &[&Path],
    moves: &[Move],
    max_depth: Option<u8>,
    last_layer_mode: bool,
    cancel: &AtomicBool,
    app: &AppHandle,
) -> Result<Option<Arc<BloomTable>>, String> {
    let mut candidates = Vec::new();
    for root in roots {
        for info in discover(root)? {
            if info.last_layer_mode == last_layer_mode
                && info
                    .depth
                    .is_some_and(|d| max_depth.is_none_or(|max| d <= max))
                && moves
                    .iter()
                    .all(|m| info.moves.iter().any(|name| name == m.name()))
            {
                candidates.push((root.join(&info.files[0]), info));
            }
        }
    }
    candidates.sort_by(|a, b| {
        b.1.depth
            .cmp(&a.1.depth)
            .then(a.1.moves.len().cmp(&b.1.moves.len()))
    });
    let mut chosen: Vec<CachedBloom> = Vec::new();
    for (path, info) in candidates {
        if let Some(primary) = chosen.first() {
            if info.depth.unwrap() > 12 || info.depth.unwrap() >= primary.table.depth() {
                continue;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("solve cancelled".into());
        }
        let resident = {
            let locked = cache.lock().map_err(|_| "solver cache lock poisoned")?;
            locked
                .bloom
                .as_ref()
                .into_iter()
                .chain(locked.secondary_bloom.as_ref())
                .find(|cached| cached.path == path)
                .map(|cached| cached.table.clone())
        };
        let table = if let Some(table) = resident {
            table
        } else {
            emit_line(
                app,
                "info",
                &format!(
                    "loading {}depth-{} Bloom pruning...",
                    if last_layer_mode { "last-layer " } else { "" },
                    info.depth.unwrap()
                ),
            );
            match BloomTable::load(&path, Some(cancel)) {
                Ok(table) => Arc::new(table),
                Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
                Err(error) => {
                    emit_line(
                        app,
                        "info",
                        &format!("skipping invalid Bloom table: {error}"),
                    );
                    continue;
                }
            }
        };
        chosen.push(CachedBloom { path, table });
        if chosen.len() == 2 {
            break;
        }
    }
    let result = match chosen.as_slice() {
        [primary, secondary] => {
            emit_line(
                app,
                "info",
                &format!(
                    "using Bloom pruning at depths {} and {}",
                    primary.table.depth(),
                    secondary.table.depth()
                ),
            );
            Some(BloomTable::paired(
                primary.table.clone(),
                secondary.table.clone(),
            )?)
        }
        [primary] => {
            emit_line(
                app,
                "info",
                &format!("using cached depth-{} Bloom pruning", primary.table.depth()),
            );
            Some(primary.table.clone())
        }
        _ => None,
    };
    let mut locked = cache.lock().map_err(|_| "solver cache lock poisoned")?;
    locked.bloom = if chosen.is_empty() {
        None
    } else {
        Some(chosen.remove(0))
    };
    locked.secondary_bloom = chosen.pop();
    Ok(result)
}
