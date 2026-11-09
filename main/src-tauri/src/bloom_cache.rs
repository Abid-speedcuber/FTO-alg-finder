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
}

#[tauri::command]
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
    }
    let cancels = solver_state.current_cancel.clone();
    let cache = solver_state.cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = with_gpu_generation(request.use_gpu, &app, || {
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
    if !(1..=20).contains(&request.depth) || request.max_memory_mib == 0 {
        return Err("Bloom depth must be 1–20 and memory budget must be at least 1 MiB".into());
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let tables = load_transition_tables(
        cache,
        &transition_dir(app, workspace).join("transition-table.tran"),
        app,
    )?;
    emit_line(
        app,
        "info",
        &format!(
            "generating {}depth-{} Bloom table for {} (memory cap {} MiB)",
            if request.last_layer_mode {
                "last-layer "
            } else {
                ""
            },
            request.depth,
            format_move_list(&moves),
            request.max_memory_mib
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
            "generated {}depth-{} Bloom table ({:.2} MiB, estimated false positives {:.3}%)",
            if request.last_layer_mode {
                "last-layer "
            } else {
                ""
            },
            info.depth,
            info.bytes as f64 / (1024. * 1024.),
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
    let mut candidates = discover(root)?;
    candidates.retain(|info| {
        info.last_layer_mode == last_layer_mode
            && info
                .depth
                .is_some_and(|d| max_depth.is_none_or(|max| d <= max))
            && moves
                .iter()
                .all(|m| info.moves.iter().any(|name| name == m.name()))
    });
    candidates.sort_by(|a, b| {
        b.depth
            .cmp(&a.depth)
            .then(a.moves.len().cmp(&b.moves.len()))
    });
    for candidate in candidates {
        if cancel.load(Ordering::Relaxed) {
            return Err("solve cancelled".into());
        }
        let path = root.join(&candidate.files[0]);
        {
            let locked = cache.lock().map_err(|_| "solver cache lock poisoned")?;
            if let Some(existing) = &locked.bloom {
                if existing.path == path {
                    emit_line(
                        app,
                        "info",
                        &format!(
                            "using cached {}depth-{} Bloom pruning",
                            if last_layer_mode { "last-layer " } else { "" },
                            existing.table.depth()
                        ),
                    );
                    return Ok(Some(existing.table.clone()));
                }
            }
        }
        emit_line(
            app,
            "info",
            &format!(
                "loading {}depth-{} Bloom pruning...",
                if last_layer_mode { "last-layer " } else { "" },
                candidate.depth.unwrap()
            ),
        );
        match BloomTable::load(&path, Some(cancel)) {
            Ok(table) => {
                let table = Arc::new(table);
                cache
                    .lock()
                    .map_err(|_| "solver cache lock poisoned")?
                    .bloom = Some(CachedBloom {
                    path,
                    table: table.clone(),
                });
                return Ok(Some(table));
            }
            Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
            Err(error) => emit_line(
                app,
                "info",
                &format!("skipping invalid Bloom table: {error}"),
            ),
        }
    }
    // Do not retain an unrelated instance's large table indefinitely.
    cache
        .lock()
        .map_err(|_| "solver cache lock poisoned")?
        .bloom = None;
    Ok(None)
}
