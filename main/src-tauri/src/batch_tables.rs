//! Owned per-move-set acceleration files. Persistent user tables are never deleted.
use super::*;
use fto_core::bloom::{BloomMode, BloomTable, GenerateConfig};

const MIB: u64 = 1024 * 1024;
pub(super) const DEEP_BATCH: u8 = 14;

pub(super) fn worthwhile(cases: usize, depths: Option<&[u8]>) -> bool {
    cases >= 40 || depths.is_some_and(|depths| depths.iter().any(|d| *d >= DEEP_BATCH))
}
fn memory_budget(requested: u32, available: u64) -> u32 {
    if requested != 0 {
        return requested;
    }
    if available == 0 {
        return 256;
    }
    (available.saturating_sub((available / 4).max(1024 * MIB)) / MIB).min(u64::from(u32::MAX))
        as u32
}
fn capacity_error(message: &str) -> bool {
    message.contains("Bloom table is full")
        || message.contains("memory")
        || message.contains("RAM")
        || message.contains("address space")
}

pub(super) struct Prepared {
    pub pruning: Option<Arc<SolverPruning>>,
    pub ll_pruning: Option<Arc<PatternDatabases>>,
}
pub(super) struct TemporaryTables<'a> {
    root: PathBuf,
    cache: &'a Mutex<SolverCache>,
    owns_pruning: bool,
    owns_ll: bool,
    pub bloom_root: Option<PathBuf>,
}
impl<'a> TemporaryTables<'a> {
    pub fn new(cache: &'a Mutex<SolverCache>) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            root: std::env::temp_dir().join(format!("fto-batch-{}-{nonce}", std::process::id())),
            cache,
            owns_pruning: false,
            owns_ll: false,
            bloom_root: None,
        }
    }
    pub fn prepare(
        &mut self,
        tables: &TransitionTables,
        persistent: &Path,
        allowed: &[Move],
        instance: &[Move],
        last_layer: bool,
        boost: bool,
        requested_mib: u32,
        max_depth: Option<u8>,
        threads: usize,
        cancel: &Arc<AtomicBool>,
        app: &AppHandle,
    ) -> Result<Prepared, String> {
        if boost {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "solver cache lock poisoned")?;
            cache.pruning = None;
            cache.ll_pruning = None;
            cache.bloom = None;
        }
        let budget = memory_budget(
            requested_mib,
            fto_core::acceleration::available_memory_bytes(),
        );
        let mut selected = allowed.to_vec();
        let mut fallback = instance.to_vec();
        if last_layer {
            ensure_ll_free_u_moves(&mut selected);
            ensure_ll_free_u_moves(&mut fallback);
        }
        selected.sort_unstable_by_key(|m| m.idx());
        let cached = select_pruning_move_set(persistent, &selected, &fallback, false, last_layer)?;
        // Reserve generation workspace as well as the two resident pattern arrays.
        let estimate = CandidateSpec::new(vec![
            Component::Edge3,
            if last_layer {
                Component::LlUfCenterA
            } else {
                Component::UfCenter3
            },
        ])
        .size()
        .unwrap_or(usize::MAX)
        .saturating_add(
            CandidateSpec::new(vec![
                Component::Corner,
                if last_layer {
                    Component::LlUfCenterB
                } else {
                    Component::UfCenter3
                },
            ])
            .size()
            .unwrap_or(usize::MAX),
        );
        let temporary = boost
            && cached != selected
            && (estimate as u64).saturating_mul(2) <= u64::from(budget) * MIB;
        let root = if temporary {
            fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
            if last_layer {
                self.owns_ll = true;
            } else {
                self.owns_pruning = true;
            }
            emit_line(
                app,
                "info",
                &format!(
                    "building temporary pruning for {} (table budget {budget} MiB)",
                    format_move_list(allowed)
                ),
            );
            &self.root
        } else {
            persistent
        };
        if boost && cached != selected && !temporary {
            emit_line(app, "info", "temporary pattern tables do not fit the budget; using the instance's compatible tables");
        }
        let load = |root: &Path, restricted| -> Result<Prepared, String> {
            if last_layer {
                Ok(Prepared {
                    pruning: None,
                    ll_pruning: Some(load_ll_solver_pruning(
                        self.cache,
                        tables,
                        root,
                        pruning_progress_interval(allowed.len()),
                        allowed,
                        instance,
                        restricted,
                        threads,
                        cancel,
                        app,
                    )?),
                })
            } else {
                Ok(Prepared {
                    pruning: Some(load_solver_pruning(
                        self.cache,
                        tables,
                        root,
                        pruning_progress_interval(allowed.len()),
                        allowed,
                        instance,
                        restricted,
                        threads,
                        cancel,
                        app,
                    )?),
                    ll_pruning: None,
                })
            }
        };
        let prepared = match load(root, temporary) {
            Ok(prepared) => prepared,
            Err(error)
                if temporary && capacity_error(&error) && !cancel.load(Ordering::Relaxed) =>
            {
                emit_line(app, "info", &format!("temporary pattern build could not fit: {error}; using reusable instance tables"));
                load(persistent, false)?
            }
            Err(error) => return Err(error),
        };
        if boost {
            let used = prepared.pruning.as_ref().map_or(0, |p| p.total_bytes())
                + prepared.ll_pruning.as_ref().map_or(0, |p| p.total_bytes());
            self.build_bloom(
                tables,
                persistent,
                allowed,
                last_layer,
                budget,
                used as u64,
                max_depth,
                cancel,
                app,
            )?;
        }
        Ok(prepared)
    }
    fn build_bloom(
        &mut self,
        tables: &TransitionTables,
        persistent: &Path,
        moves: &[Move],
        last_layer: bool,
        budget: u32,
        pattern_bytes: u64,
        max_depth: Option<u8>,
        cancel: &AtomicBool,
        app: &AppHandle,
    ) -> Result<(), String> {
        let types: HashSet<_> = moves
            .iter()
            .map(|m| m.idx().min(m.inverse().idx()))
            .collect();
        if types.len() > 5 {
            emit_line(app, "info", "automatic Bloom generation is limited to five move types; using saved Bloom tables");
            return Ok(());
        }
        let limit = max_depth.unwrap_or(u8::MAX);
        if limit == 0 {
            return Ok(());
        }
        let existing = bloom_cache::load(
            self.cache,
            persistent,
            moves,
            Some(limit),
            last_layer,
            cancel,
            app,
        )?;
        if existing
            .as_ref()
            .is_some_and(|table| table.depth() >= limit)
        {
            return Ok(());
        }
        let start = existing
            .as_ref()
            .map_or(1, |table| table.depth().saturating_add(1));
        drop(existing);
        if start > limit {
            return Ok(());
        }
        // Keep the previous completed filter on disk while probing a deeper one.
        self.cache
            .lock()
            .map_err(|_| "solver cache lock poisoned")?
            .bloom = None;
        let mode = if last_layer {
            BloomMode::LastLayer
        } else {
            BloomMode::FullPosition
        };
        // Two filters' worth leaves space for GPU/work buffers during generation.
        let cap = ((u64::from(budget) * MIB).saturating_sub(pattern_bytes) / 2 / MIB)
            .min(u64::from(u32::MAX)) as u32;
        if grow_bloom(
            tables,
            moves,
            mode,
            start,
            limit,
            cap,
            &self.root,
            cancel,
            &|kind, text| emit_line(app, kind, text),
        )?
        .is_some()
        {
            self.bloom_root = Some(self.root.clone());
        }
        Ok(())
    }
}
fn grow_bloom(
    tables: &TransitionTables,
    moves: &[Move],
    mode: BloomMode,
    start: u8,
    limit: u8,
    cap: u32,
    root: &Path,
    cancel: &AtomicBool,
    log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<Option<PathBuf>, String> {
    if cap == 0 {
        log(
            "info",
            "no Bloom workspace remains in the table budget; retaining saved tables",
        );
        return Ok(None);
    }
    let mut previous: Option<PathBuf> = None;
    for depth in start..=limit {
        if cancel.load(Ordering::Relaxed) {
            return Err("solve cancelled".into());
        }
        log(
            "info",
            &format!("trying temporary Bloom depth {depth}, {cap} MiB filter cap"),
        );
        let progress = |p: &fto_core::bloom::BloomProgress| {
            log(
                "progress",
                &format!(
                    "temporary Bloom depth {depth}: {}/{} paths",
                    p.visited, p.total
                ),
            )
        };
        let generated = BloomTable::generate(
            tables,
            moves,
            GenerateConfig {
                mode,
                depth,
                max_memory_mib: cap,
            },
            Some(cancel),
            Some(&progress),
        );
        let table = match generated {
            Ok(table) => table,
            Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
            Err(error) if capacity_error(&error) => {
                log( "info", &format!("stopping Bloom growth at depth {depth}: {error}; keeping the last completed table"));
                break;
            }
            Err(error) => {
                log(
                    "info",
                    &format!("temporary Bloom build skipped: {error}; using completed tables"),
                );
                break;
            }
        };
        let path = root
            .join("bloom-v1")
            .join(fto_core::bloom::cache_file_name(moves, depth, mode));
        if let Err(error) = table.save(&path, Some(cancel)) {
            if cancel.load(Ordering::Relaxed) {
                return Err(error);
            }
            log( "info", &format!("could not save deeper Bloom table: {error}; retaining the last completed table"));
            break;
        }
        drop(table);
        if let Some(prior) = previous.replace(path) {
            fs::remove_file(prior).map_err(|e| e.to_string())?;
        }
        log("info", &format!("temporary Bloom depth {depth} completed"));
    }
    Ok(previous)
}

impl Drop for TemporaryTables<'_> {
    fn drop(&mut self) {
        if let Ok(mut cache) = self.cache.lock() {
            if self.owns_pruning {
                cache.pruning = None;
            }
            if self.owns_ll {
                cache.ll_pruning = None;
            }
            if cache
                .bloom
                .as_ref()
                .is_some_and(|cached| cached.path.starts_with(&self.root))
            {
                cache.bloom = None;
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_policy_and_budgets() {
        assert!(!worthwhile(39, None));
        assert!(worthwhile(40, None));
        assert!(!worthwhile(1, Some(&[10, 13])));
        assert!(worthwhile(1, Some(&[14])));
        assert_eq!(memory_budget(256, 20 * MIB), 256);
        assert_eq!(memory_budget(0, 8 * 1024 * MIB), 6 * 1024);
        assert_eq!(memory_budget(0, 512 * MIB), 0);
        assert!(capacity_error("Bloom table is full"));
        assert!(capacity_error("not enough available RAM"));
    }
    #[test]
    #[ignore = "uses the local transition cache and builds bounded real Bloom tables"]
    fn adaptive_growth_retains_last_complete_filter_and_optimal_solutions() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let tables = TransitionTables::load(workspace.join("cache/transition-table.tran")).unwrap();
        let cache = Mutex::new(SolverCache::default());
        let guard = TemporaryTables::new(&cache);
        let messages = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let path = grow_bloom(
            &tables,
            &Move::ALL,
            BloomMode::FullPosition,
            1,
            4,
            1,
            &guard.root,
            &cancel,
            &|_, message| messages.lock().unwrap().push(message.to_owned()),
        )
        .unwrap()
        .unwrap();
        let table = Arc::new(BloomTable::load(&path, None).unwrap());
        assert!(
            table.depth() < 4,
            "1 MiB must stop growth before depth four for all moves"
        );
        assert!(messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.contains("keeping the last completed table")));
        assert_eq!(
            fs::read_dir(guard.root.join("bloom-v1")).unwrap().count(),
            1
        );
        let start = [Move::R, Move::U, Move::F]
            .into_iter()
            .fold(FtoCubie::solved(), |c, m| c.apply(m))
            .coord();
        let config = SearchConfig {
            min_depth: 0,
            max_depth: 4,
            allowed_moves: vec![Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp],
            ..SearchConfig::default()
        };
        let plain = search::solve(start, &tables, &config);
        let bounded = search::solve(
            start,
            &tables,
            &SearchConfig {
                bloom: Some(table),
                ..config
            },
        );
        assert!(!plain.solutions.is_empty());
        assert_eq!(plain.solutions[0].len(), bounded.solutions[0].len());
        let deep_path = grow_bloom(
            &tables,
            &[Move::R],
            BloomMode::FullPosition,
            1,
            16,
            1,
            &guard.root,
            &cancel,
            &|_, _| {},
        )
        .unwrap()
        .unwrap();
        let deep = BloomTable::load(&deep_path, None).unwrap();
        assert_eq!(
            deep.depth(),
            16,
            "automatic growth must pass depth twelve when the table fits"
        );
        assert!(deep.info().bytes < MIB);
        let deepest = BloomTable::generate(
            &tables,
            &[Move::R],
            GenerateConfig {
                mode: BloomMode::FullPosition,
                depth: 255,
                max_memory_mib: 1,
            },
            None,
            None,
        )
        .unwrap();
        let path_255 = guard.root.join("depth-255.bloom");
        deepest.save(&path_255, None).unwrap();
        assert_eq!(BloomTable::load(&path_255, None).unwrap().depth(), 255);
        assert!(BloomTable::generate(
            &tables,
            &Move::ALL,
            GenerateConfig {
                mode: BloomMode::FullPosition,
                depth: 255,
                max_memory_mib: 1
            },
            None,
            None
        )
        .unwrap_err()
        .contains("traversal is too large"));
        let spec = CandidateSpec::new(vec![Component::E0]);
        let pattern = guard.root.join("metadata.prun");
        PatternDatabase::load_or_build_with_moves_reporting(
            spec,
            &tables,
            0,
            &pattern,
            &[Move::R],
            1,
            None,
            None,
        )
        .unwrap();
        assert!(pruning::read_table_metadata(&pattern)
            .unwrap()
            .contains("moves=R"));
        cache.lock().unwrap().bloom = Some(bloom_cache::CachedBloom {
            path: path.clone(),
            table: Arc::new(BloomTable::load(&path, None).unwrap()),
        });
        let root = guard.root.clone();
        drop(guard);
        assert!(!root.exists());
        assert!(cache.lock().unwrap().bloom.is_none());
    }

    #[test]
    fn temporary_cleanup_never_removes_user_tables() {
        let cache = Mutex::new(SolverCache::default());
        let guard = TemporaryTables::new(&cache);
        fs::create_dir_all(&guard.root).unwrap();
        fs::write(guard.root.join("owned"), b"temp").unwrap();
        let path = guard.root.clone();
        let saved = path.with_extension("saved-user-table");
        fs::write(&saved, b"saved").unwrap();
        drop(guard);
        assert!(!path.exists());
        assert_eq!(fs::read(&saved).unwrap(), b"saved");
        fs::remove_file(saved).unwrap();
    }
}
