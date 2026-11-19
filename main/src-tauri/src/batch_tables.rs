//! Owned per-move-set acceleration files. Persistent user tables are never deleted.
use super::*;
use fto_core::bloom::{BloomMode, BloomTable, GenerateConfig};

const MIB: u64 = 1024 * 1024;
const MAX_AUTO_BLOOM_DEPTH: u8 = 16;
const SECONDARY_BLOOM_DEPTH: u8 = 12;
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
    pub partial_root: Option<PathBuf>,
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
            partial_root: None,
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
            cache.secondary_bloom = None;
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
                    "building temporary pruning for {} (table budget {})",
                    format_move_list(allowed),
                    format_bytes(u64::from(budget) * MIB)
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
    pub fn prepare_partial(
        &mut self,
        tables: &TransitionTables,
        persistent: &Path,
        mask: &PartialMask,
        moves: &[Move],
        requested_mib: u32,
        max_depth: Option<u8>,
        threads: usize,
        cancel: &AtomicBool,
        app: &AppHandle,
    ) -> Result<(), String> {
        let budget = u64::from(memory_budget(
            requested_mib,
            fto_core::acceleration::available_memory_bytes(),
        )) * MIB;
        let out = self.root.join("partial-goal-v1");
        if prepare_partial_tables(
            tables,
            persistent,
            &out,
            mask,
            moves,
            budget,
            max_depth,
            threads,
            cancel,
            &|kind, text| emit_line(app, kind, text),
        )? {
            self.partial_root = Some(out);
        }
        Ok(())
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
        let limit = max_depth
            .unwrap_or(MAX_AUTO_BLOOM_DEPTH)
            .min(MAX_AUTO_BLOOM_DEPTH);
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
        let missing_secondary = existing.as_ref().is_some_and(|table| {
            table.depth() > SECONDARY_BLOOM_DEPTH
                && table.secondary_depth() != Some(SECONDARY_BLOOM_DEPTH)
        });
        if existing
            .as_ref()
            .is_some_and(|table| table.depth() >= limit)
            && !missing_secondary
        {
            return Ok(());
        }
        let start = existing
            .as_ref()
            .map_or(1, |table| table.depth().saturating_add(1));
        drop(existing);
        // Keep the previous completed filter on disk while probing a deeper one.
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "solver cache lock poisoned")?;
            cache.bloom = None;
            cache.secondary_bloom = None;
        }
        let mode = if last_layer {
            BloomMode::LastLayer
        } else {
            BloomMode::FullPosition
        };
        // Two filters' worth leaves space for GPU/work buffers during generation.
        let cap = ((u64::from(budget) * MIB).saturating_sub(pattern_bytes) / 2 / MIB)
            .min(u64::from(u32::MAX)) as u32;
        if missing_secondary
            && grow_bloom(
                tables,
                moves,
                mode,
                SECONDARY_BLOOM_DEPTH,
                SECONDARY_BLOOM_DEPTH,
                cap,
                &self.root,
                cancel,
                &|kind, text| emit_line(app, kind, text),
            )?
            .is_some()
        {
            self.bloom_root = Some(self.root.clone());
        }
        if start > limit {
            return Ok(());
        }
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
fn prepare_partial_tables(
    tables: &TransitionTables,
    persistent: &Path,
    out: &Path,
    mask: &PartialMask,
    moves: &[Move],
    budget: u64,
    max_depth: Option<u8>,
    threads: usize,
    cancel: &AtomicBool,
    log: &(dyn Fn(&str, &str) + Send + Sync),
) -> Result<bool, String> {
    use fto_core::partial_goal;
    let workspace = (budget / 4).min(4096 * MIB);
    if workspace < MIB {
        log(
            "info",
            "partial preparation does not fit the table budget; using saved partial tables",
        );
        return Ok(false);
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    // Bring compatible saved tables into the owned namespace without altering them.
    // Hard links avoid copying large files when both locations share a filesystem.
    let plan = match partial_goal::plan(mask, tables, workspace.min(isize::MAX as u64) as usize, Some(cancel)) {
        Ok(plan) => plan,
        Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
        Err(error) => {
            log(
                "info",
                &format!("partial preparation skipped: {error}; using reusable partial heuristics"),
            );
            return Ok(false);
        }
    };
    let limit = max_depth
        .unwrap_or(MAX_AUTO_BLOOM_DEPTH)
        .min(MAX_AUTO_BLOOM_DEPTH);
    let components: Vec<_> = plan
        .spec
        .components
        .iter()
        .map(|component| component.name().to_owned())
        .collect();
    let mut candidates = partial_goal::discover(persistent);
    candidates.retain(|(_, meta)| {
        meta.compatible(mask, moves)
            && meta
                .depth
                .is_none_or(|depth| depth <= limit && meta.components == components)
    });
    candidates.sort_by_key(|(_, meta)| {
        std::cmp::Reverse((meta.mask == *mask, meta.version, meta.entries, meta.depth))
    });
    let (mut patterns, mut blooms) = (0, 0);
    let mut saved_depth = 0;
    let mut saved_secondary = 0;
    for (id, meta) in candidates {
        if meta.depth.is_some() {
            if blooms == 2
                || (blooms == 1
                    && (meta.depth.unwrap() > SECONDARY_BLOOM_DEPTH
                        || meta.depth.unwrap() >= saved_depth))
            {
                continue;
            }
        } else if patterns == 2 {
            continue;
        }
        let ext = if meta.depth.is_some() {
            "gbloom"
        } else {
            "prun"
        };
        let files = [format!("{id}.{ext}"), format!("{id}.json")];
        if !persistent.join(&files[0]).is_file() {
            continue;
        }
        let mut copied = true;
        for file in &files {
            let source = persistent.join(file);
            let target = out.join(file);
            if fs::hard_link(&source, &target).is_err() && fs::copy(&source, &target).is_err() {
                copied = false;
                break;
            }
        }
        if copied {
            if let Some(depth) = meta.depth {
                if blooms == 0 {
                    saved_depth = depth;
                } else {
                    saved_secondary = depth;
                }
                blooms += 1;
            } else {
                patterns += 1;
            }
        } else {
            for file in &files {
                let _ = fs::remove_file(out.join(file));
            }
        }
    }
    let report = |p: pruning::PruningProgress| log("progress", &format_pruning_progress(&p));
    let pattern = partial_goal::metadata(&plan, mask, moves, None);
    let pattern_id = partial_goal::key(&pattern);
    let saved_pattern = [format!("{pattern_id}.prun"), format!("{pattern_id}.json")];
    // The desired projection may not be among the two broad tables seeded above.
    if saved_pattern
        .iter()
        .all(|file| persistent.join(file).is_file())
    {
        for file in &saved_pattern {
            if !out.join(file).exists()
                && fs::hard_link(persistent.join(file), out.join(file)).is_err()
            {
                fs::copy(persistent.join(file), out.join(file)).map_err(|e| e.to_string())?;
            }
        }
        log(
            "info",
            "reusing saved partial pruning for this move set and goal",
        );
    } else {
        log(
            "info",
            &format!(
                "preparing shared partial pruning: {} ({})",
                plan.spec.name(),
                format_bytes(plan.entries as u64)
            ),
        );
        match partial_goal::generate(
            &plan,
            &pattern,
            tables,
            &out,
            threads,
            Some(cancel),
            Some(&report),
            0,
        ) {
            Ok(()) => {}
            Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
            Err(error) => log(
                "info",
                &format!(
                    "partial pattern build skipped: {error}; using existing partial heuristics"
                ),
            ),
        }
    }
    let cap = (budget / 4 / MIB).min(u64::from(u32::MAX)) as u32;
    if cap == 0 || saved_depth == 255 {
        return Ok(true);
    }
    let mut depths: Vec<_> = (saved_depth.saturating_add(1)..=limit).collect();
    if saved_depth > SECONDARY_BLOOM_DEPTH && saved_secondary != SECONDARY_BLOOM_DEPTH {
        depths.insert(0, SECONDARY_BLOOM_DEPTH);
    }
    let mut previous: Option<(u8, String)> = None;
    let mut secondary: Option<(u8, String)> = None;
    for depth in depths {
        let meta = partial_goal::metadata(&plan, mask, moves, Some(depth));
        log(
            "info",
            &format!("trying temporary partial Bloom depth {depth} ({} filter cap)", format_bytes(u64::from(cap) * MIB)),
        );
        match partial_goal::generate_with_reached_depth(
            &plan,
            &meta,
            tables,
            &out,
            threads,
            Some(cancel),
            Some(&report),
            cap,
        ) {
            Ok(reached) => {
                let id = partial_goal::key(&meta);
                if let Some((prior_depth, old)) = previous.replace((depth, id)) {
                    let remove = if prior_depth <= SECONDARY_BLOOM_DEPTH {
                        secondary.replace((prior_depth, old)).map(|(_, id)| id)
                    } else {
                        Some(old)
                    };
                    if let Some(old) = remove {
                        for ext in ["gbloom", "json"] {
                            let _ = fs::remove_file(out.join(format!("{old}.{ext}")));
                        }
                    }
                }
                if reached.is_some_and(|reached| reached < depth) {
                    log("info","partial Bloom projection is fully covered; deeper tables would add no pruning");
                    break;
                }
            }
            Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
            Err(error) => {
                log("info",&format!("stopping partial Bloom growth: {error}; retaining the last completed table"));
                break;
            }
        }
    }
    Ok(true)
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
    let mut previous: Option<(u8, PathBuf)> = None;
    let mut secondary: Option<(u8, PathBuf)> = None;
    for depth in start..=limit.min(MAX_AUTO_BLOOM_DEPTH) {
        if cancel.load(Ordering::Relaxed) {
            return Err("solve cancelled".into());
        }
        log(
            "info",
            &format!("trying temporary Bloom depth {depth}, {} filter cap", format_bytes(u64::from(cap) * MIB)),
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
        let path = root
            .join("bloom-v1")
            .join(fto_core::bloom::cache_file_name(moves, depth, mode));
        let saved = if path.is_file() {
            match BloomTable::load(&path, Some(cancel)) {
                Ok(table) => Some(table),
                Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
                Err(_) => None,
            }
        } else {
            None
        };
        let generated = if let Some(table) = saved
            .filter(|table| table.depth() == depth && table.mode() == mode && table.covers(moves))
        {
            log(
                "info",
                &format!("reusing completed temporary Bloom depth {depth}"),
            );
            Ok(table)
        } else {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            BloomTable::generate(
                tables,
                moves,
                GenerateConfig {
                    mode,
                    depth,
                    max_memory_mib: cap,
                },
                Some(cancel),
                Some(&progress),
            )
        };
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
        let saved = if path.is_file() {
            Ok(())
        } else {
            table.save(&path, Some(cancel))
        };
        if let Err(error) = saved {
            if cancel.load(Ordering::Relaxed) {
                return Err(error);
            }
            log( "info", &format!("could not save deeper Bloom table: {error}; retaining the last completed table"));
            break;
        }
        drop(table);
        if let Some((prior_depth, prior)) = previous.replace((depth, path)) {
            if prior_depth <= SECONDARY_BLOOM_DEPTH {
                if let Some((_, old)) = secondary.replace((prior_depth, prior)) {
                    fs::remove_file(old).map_err(|e| e.to_string())?;
                }
            } else {
                fs::remove_file(prior).map_err(|e| e.to_string())?;
            }
        }
        log("info", &format!("temporary Bloom depth {depth} completed"));
    }
    Ok(previous.map(|(_, path)| path))
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
        if let Ok(mut cache) = self.cache.lock() {
            if cache
                .secondary_bloom
                .as_ref()
                .is_some_and(|cached| cached.path.starts_with(&self.root))
            {
                cache.secondary_bloom = None;
            }
        }
        if self.partial_root.is_some() {
            fto_core::partial_goal::clear_cache();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "uses the local transition cache to prepare real partial tables"]
    fn automatic_partial_tables_keep_saved_files_and_entitled_goals_separate() {
        use fto_core::partial_goal;
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let tables = Arc::new(
            TransitionTables::load(workspace.join("cache/transition-table.tran")).unwrap(),
        );
        partial_goal::set_transition_tables(tables.clone());
        let cache = Mutex::new(SolverCache::default());
        let saved = TemporaryTables::new(&cache);
        let mut temporary = TemporaryTables::new(&cache);
        let source = saved.root.join("saved-partial");
        let out = temporary.root.join("partial-goal-v1");
        let mut mask = PartialMask::all();
        mask.corners = [false; 6];
        mask.edges = [false; 12];
        mask.uf_centers = [false; 12];
        mask.rl_centers = [false; 12];
        mask.uf_centers[0] = true;
        mask.uf_entitled_slots[0] = 1;
        let moves = [Move::R, Move::Rp, Move::U, Move::Up];
        let cancel = AtomicBool::new(false);
        assert!(prepare_partial_tables(
            &tables,
            &saved.root.join("missing"),
            &source,
            &mask,
            &moves,
            8 * MIB,
            Some(2),
            2,
            &cancel,
            &|_, _| {}
        )
        .unwrap());
        let saved_ids: Vec<_> = partial_goal::discover(&source)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(saved_ids.len() >= 2);
        let reused = temporary.root.join("partial-reuse");
        let events = Mutex::new(Vec::new());
        assert!(prepare_partial_tables(
            &tables,
            &source,
            &reused,
            &mask,
            &moves,
            8 * MIB,
            Some(2),
            2,
            &cancel,
            &|kind, message| events
                .lock()
                .unwrap()
                .push((kind.to_owned(), message.to_owned()))
        )
        .unwrap());
        assert!(
            !events
                .lock()
                .unwrap()
                .iter()
                .any(|(kind, _)| kind == "progress"),
            "completed saved pattern/Bloom tables must not be generated again"
        );
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|(_, message)| message.contains("reusing saved partial")));
        assert!(prepare_partial_tables(
            &tables,
            &source,
            &out,
            &mask,
            &moves,
            8 * MIB,
            Some(3),
            2,
            &cancel,
            &|_, _| {}
        )
        .unwrap());
        temporary.partial_root = Some(out.clone());
        let loaded = partial_goal::load(&out, &mask, &moves, None).unwrap();
        assert!(loaded.0.len() > 0 && loaded.1.is_some());
        let filter = loaded.1.as_ref().unwrap();
        assert!(filter.secondary_depth().is_some_and(|d| d < filter.depth()));
        drop(loaded);
        let mut other = mask.clone();
        other.uf_entitled_slots[0] = 2;
        let rejected = partial_goal::load(&out, &other, &moves, None).unwrap();
        assert_eq!(rejected.0.len(), 0);
        assert!(rejected.1.is_none());
        drop(rejected);
        let temp_root = temporary.root.clone();
        drop(temporary);
        assert!(!temp_root.exists());
        for id in saved_ids {
            assert!(source.join(format!("{id}.json")).exists());
        }
        assert!(partial_goal::load(&source, &mask, &moves, None)
            .unwrap()
            .1
            .is_some());
        partial_goal::clear_cache();
        drop(saved);
    }

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
            usize::from(table.depth().min(2))
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
            255,
            1,
            &guard.root,
            &cancel,
            &|_, _| {},
        )
        .unwrap()
        .unwrap();
        let modified = fs::metadata(&deep_path).unwrap().modified().unwrap();
        let reused = grow_bloom(
            &tables,
            &[Move::R],
            BloomMode::FullPosition,
            16,
            16,
            1,
            &guard.root,
            &cancel,
            &|_, _| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!(reused, deep_path);
        assert_eq!(
            fs::metadata(&deep_path).unwrap().modified().unwrap(),
            modified,
            "reusing a complete filter must not rewrite it"
        );
        let deep = BloomTable::load(&deep_path, None).unwrap();
        assert_eq!(
            deep.depth(),
            16,
            "automatic growth must pass depth twelve when the table fits"
        );
        assert!(deep.info().bytes < MIB);
        let mut retained: Vec<_> = fs::read_dir(guard.root.join("bloom-v1"))
            .unwrap()
            .map(|entry| BloomTable::read_info(&entry.unwrap().path()).unwrap())
            .filter(|info| info.moves == vec![Move::R])
            .map(|info| info.depth)
            .collect();
        retained.sort();
        assert_eq!(
            retained,
            vec![12, 16],
            "retain both checkpoints and cap automatic growth"
        );
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
