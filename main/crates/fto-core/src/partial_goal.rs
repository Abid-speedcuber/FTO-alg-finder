//! Persistent joint-coordinate pruning for partially defined goals.
//! Unknown pieces are relaxed in the goal seeds, rather than arbitrarily solved.
use crate::{
    coord::{self, FtoCoord},
    moves::Move,
    partial::PartialMask,
    pruning::{self, CandidateSpec, Component, PatternDatabase, PatternDatabases, PruningReporter},
    tables::TransitionTables,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub version: u8,
    pub model: u64,
    pub mask: PartialMask,
    pub components: Vec<String>,
    pub moves: String,
    pub depth: Option<u8>,
    pub entries: usize,
}
impl Metadata {
    pub fn spec(&self) -> Option<CandidateSpec> {
        let parts = self
            .components
            .iter()
            .map(|s| Component::parse(s))
            .collect::<Option<Vec<_>>>()?;
        if parts.is_empty() {
            None
        } else {
            Some(CandidateSpec::new(parts))
        }
    }
    pub fn compatible(&self, mask: &PartialMask, moves: &[Move]) -> bool {
        (self.version == 1 || self.version == 2)
            && (self.version == 1
                || ((self.mask.uf_entitled_slots == [0; 4]
                    || self.mask.uf_entitled_slots == mask.uf_entitled_slots)
                    && (self.mask.rl_entitled_slots == [0; 4]
                        || self.mask.rl_entitled_slots == mask.rl_entitled_slots)
                    && (self.mask.uf_center_targets == [None; 4]
                        || self.mask.uf_center_targets == mask.uf_center_targets)
                    && (self.mask.rl_center_targets == [None; 4]
                        || self.mask.rl_center_targets == mask.rl_center_targets)))
            && self.model == crate::bloom::model_signature()
            && self.mask.last_layer_centers == mask.last_layer_centers
            && self
                .mask
                .corners
                .iter()
                .zip(mask.corners)
                .all(|(&a, b)| !a || b)
            && self
                .mask
                .edges
                .iter()
                .zip(mask.edges)
                .all(|(&a, b)| !a || b)
            && [&self.mask.uf_centers, &self.mask.rl_centers]
                .iter()
                .zip([&mask.uf_centers, &mask.rl_centers])
                .all(|(a, b)| {
                    (0..4).all(|c| {
                        a[c * 3..c * 3 + 3].iter().filter(|&&v| v).count()
                            <= b[c * 3..c * 3 + 3].iter().filter(|&&v| v).count()
                    })
                })
            && pruning::parse_move_set_suffix(&self.moves)
                .is_some_and(|set| moves.iter().all(|m| set.contains(m)))
    }
}
pub struct Plan {
    pub spec: CandidateSpec,
    pub goals: Vec<Vec<u32>>,
    pub entries: usize,
    pub roots: usize,
}
fn active(cancel: Option<&AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err("table generation cancelled".into())
    } else {
        Ok(())
    }
}
fn map_label(piece: usize, counts: &[u8]) -> u8 {
    let mut end = 0;
    for (label, &count) in counts.iter().enumerate() {
        end += count as usize;
        if piece < end {
            return label as u8;
        }
    }
    unreachable!()
}
fn accepts(component: Component, value: u32, mask: &PartialMask) -> bool {
    if component == Component::Corner {
        let (cp, co) = coord::unrank_corner(value as u16);
        return (0..6).all(|p| !mask.corners[p] || (cp[p] as usize == p && co[p] == 0));
    }
    if let Some(bit) = [Component::E0, Component::E1, Component::E2, Component::E3]
        .iter()
        .position(|&c| c == component)
    {
        let bitmap = coord::unrank_choice6(value as u16);
        let signatures = [0u8, 1, 2, 3, 4, 5, 10, 11, 12, 13, 14, 15];
        return (0..12)
            .all(|p| !mask.edges[p] || ((bitmap >> p) & 1) as u8 == (signatures[p] >> bit) & 1);
    }
    let (counts, cared, center): (&[u8], &[bool; 12], bool) = match component {
        Component::Edge3 => (&coord::EDGE3_COUNTS, &mask.edges, false),
        Component::Edge4 => (&coord::EDGE4_COUNTS, &mask.edges, false),
        Component::UfCenter => (&coord::CENTER4_COUNTS, &mask.uf_centers, true),
        Component::RlCenter => (&coord::CENTER4_COUNTS, &mask.rl_centers, true),
        Component::UfCenter2 => (&coord::CENTER2_COUNTS, &mask.uf_centers, true),
        Component::RlCenter2 => (&coord::CENTER2_COUNTS, &mask.rl_centers, true),
        Component::UfCenter3 => (&coord::CENTER3_COUNTS, &mask.uf_centers, true),
        Component::RlCenter3 => (&coord::CENTER3_COUNTS, &mask.rl_centers, true),
        _ => return true,
    };
    let colors = coord::unrank_multiset_colors(value, counts);
    if !center {
        return (0..12).all(|p| !cared[p] || colors[p] == map_label(p, counts));
    }
    let (entitled, targets) = match component {
        Component::UfCenter | Component::UfCenter2 | Component::UfCenter3 => {
            (&mask.uf_entitled_slots, &mask.uf_center_targets)
        }
        _ => (&mask.rl_entitled_slots, &mask.rl_center_targets),
    };
    // Identity information is relaxed by the color projection, but every true
    // goal must still put enough of this color into its entitled home slots.
    (0..4).all(|color| {
        let required = cared[color * 3..color * 3 + 3]
            .iter()
            .filter(|&&v| v)
            .count();
        let label = map_label(color * 3, counts);
        if colors[color * 3..color * 3 + 3]
            .iter()
            .filter(|&&v| v == label)
            .count()
            < required
        {
            return false;
        }
        let slots = entitled[color];
        let matched = (0..12)
            .filter(|&slot| slots & (1 << slot) != 0 && colors[slot] == label)
            .count();
        if matched < required.min(slots.count_ones() as usize) {
            return false;
        }
        required == 0 || targets[color].is_none_or(|slot| colors[slot as usize] == label)
    })
}

pub fn plan(
    mask: &PartialMask,
    tables: &TransitionTables,
    max_bytes: usize,
    cancel: Option<&AtomicBool>,
) -> Result<Plan, String> {
    use Component::*;
    let _stage = crate::acceleration::stage("partial goal planning");
    let mut choices = Vec::new();
    for component in [
        Corner, Edge3, Edge4, E0, E1, E2, E3, UfCenter3, RlCenter3, UfCenter2, RlCenter2, UfCenter,
        RlCenter,
    ] {
        let mut goals = Vec::new();
        for value in 0..component.size() as u32 {
            if value & 4095 == 0 {
                active(cancel)?;
            }
            let mut next = value;
            for _ in 0..if mask.last_layer_centers { 3 } else { 1 } {
                if accepts(component, next, mask) {
                    goals.push(value);
                    break;
                }
                next = component.next(next, Move::U, tables);
            }
        }
        if !goals.is_empty() && goals.len() < component.size() {
            choices.push((component, goals));
        }
    }
    let mut candidates = Vec::new();
    for a in 0..choices.len() {
        for b in a..choices.len() {
            let center_orbit = |c| match c {
                UfCenter | UfCenter2 | UfCenter3 => 1,
                RlCenter | RlCenter2 | RlCenter3 => 2,
                _ => 0,
            };
            if a != b
                && center_orbit(choices[a].0) != 0
                && center_orbit(choices[a].0) == center_orbit(choices[b].0)
            {
                continue;
            }
            let indices = if a == b { vec![a] } else { vec![a, b] };
            let components: Vec<_> = indices.iter().map(|&i| choices[i].0).collect();
            let spec = CandidateSpec::new(components);
            let Some(entries) = spec.size().filter(|&n| n <= max_bytes) else {
                continue;
            };
            let roots = indices
                .iter()
                .map(|&i| choices[i].1.len())
                .product::<usize>();
            let information = (entries as f64 / roots as f64).ln();
            candidates.push((information, entries, roots, indices, spec));
        }
    }
    let best = candidates.iter().map(|c| c.0).fold(0f64, f64::max);
    // Prefer a genuinely joint, large projection, while retaining at least 90%
    // of the most selective goal available within the requested table budget.
    candidates.retain(|c| c.0 >= best * 0.9);
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.total_cmp(&a.0)));
    let Some((_, entries, roots, indices, spec)) = candidates.into_iter().next() else {
        return Err("Define at least one piece before generating a partial table.".into());
    };
    Ok(Plan {
        spec,
        goals: indices.iter().map(|&i| choices[i].1.clone()).collect(),
        entries,
        roots,
    })
}
pub fn metadata(plan: &Plan, mask: &PartialMask, moves: &[Move], depth: Option<u8>) -> Metadata {
    Metadata {
        version: 2,
        model: crate::bloom::model_signature(),
        mask: mask.clone(),
        components: plan
            .spec
            .components
            .iter()
            .map(|c| c.name().to_owned())
            .collect(),
        moves: pruning::move_set_suffix(moves),
        depth,
        entries: plan.entries,
    }
}
pub fn key(meta: &Metadata) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(meta).unwrap().hash(&mut hash);
    format!("{:016x}", hash.finish())
}
pub fn generate(
    plan: &Plan,
    meta: &Metadata,
    tables: &TransitionTables,
    out: &Path,
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
    bloom_mib: u32,
) -> Result<(), String> {
    generate_with_reached_depth(plan, meta, tables, out, threads, cancel, report, bloom_mib)
        .map(|_| ())
}

/// Returns the largest newly reached distance, or None for a cached file.
/// A result below the requested radius means the projection is fully covered.
pub fn generate_with_reached_depth(
    plan: &Plan,
    meta: &Metadata,
    tables: &TransitionTables,
    out: &Path,
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
    bloom_mib: u32,
) -> Result<Option<u8>, String> {
    active(cancel)?;
    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let id = key(meta);
    let filename = format!(
        "{id}.{}",
        if meta.depth.is_some() {
            "gbloom"
        } else {
            "prun"
        }
    );
    let path = out.join(&filename);
    if out.join(format!("{id}.json")).exists() && path.exists() {
        return Ok(None);
    }
    let moves = pruning::parse_move_set_suffix(&meta.moves).ok_or("invalid partial moves")?;
    let (stats, distances) = pruning::build_pruning_table_from_goals(
        &plan.spec,
        tables,
        plan.entries,
        250_000,
        &plan.goals,
        &moves,
        threads,
        cancel,
        report,
        meta.depth.map(|depth| depth.min(254)),
        true,
    )?;
    if meta.depth == Some(255) && stats.max_depth >= 254 {
        return Err("Partial Bloom traversal exceeds the byte-distance generator limit".into());
    }
    if meta.depth.is_some() {
        let bytes = (bloom_mib as usize)
            .checked_mul(1024 * 1024)
            .ok_or("Bloom allocation overflow")?;
        if bytes < 8 {
            return Err("Bloom memory must be at least 1 MiB".into());
        }
        crate::acceleration::check_host_allocation(bytes as u64)?;
        let mut words = vec![0u64; bytes / 8];
        for (index, &distance) in distances.iter().enumerate() {
            if index & 8191 == 0 {
                active(cancel)?;
            }
            if distance != 255 {
                insert(&mut words, index);
            }
        }
        let occupied: u64 = words.iter().map(|w| u64::from(w.count_ones())).sum();
        if (occupied as f64 / (words.len() * 64) as f64).powi(7) > 0.01 {
            return Err("Partial Bloom filter exceeds 1% false positives. Increase its memory cap or reduce depth.".into());
        }
        let pending = crate::cache_file::CacheWrite::new(&path).map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut writer =
            std::io::BufWriter::new(fs::File::create(pending.path()).map_err(|e| e.to_string())?);
        writer
            .write_all(b"FTO_PBLOOM_V1\0\0\0")
            .map_err(|e| e.to_string())?;
        let checksum = words.iter().fold(0xcbf29ce484222325u64, |h, &w| {
            (h ^ w).wrapping_mul(0x100000001b3)
        });
        writer
            .write_all(&checksum.to_le_bytes())
            .map_err(|e| e.to_string())?;
        for chunk in words.chunks(8192) {
            active(cancel)?;
            for word in chunk {
                writer
                    .write_all(&word.to_le_bytes())
                    .map_err(|e| e.to_string())?;
            }
        }
        writer.flush().map_err(|e| e.to_string())?;
        drop(writer);
        active(cancel)?;
        pending.commit().map_err(|e| e.to_string())?;
    } else {
        pruning::write_table_with_metadata_reporting(
            &path,
            &distances,
            &serde_json::to_string(meta).map_err(|e| e.to_string())?,
            report,
            "saving partial goal",
            cancel,
        )
        .map_err(|e| e.to_string())?;
    }
    active(cancel)?;
    let pending = crate::cache_file::CacheWrite::new(&out.join(format!("{id}.json")))
        .map_err(|e| e.to_string())?;
    fs::write(
        pending.path(),
        serde_json::to_vec(meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    pending.commit().map_err(|e| e.to_string())?;
    Ok(Some(stats.max_depth))
}
fn hashes(index: usize) -> (u64, u64) {
    let mut x = index as u64 ^ 0x7061727469616c31;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^= x >> 31;
    (x, x.rotate_left(29) | 1)
}
fn insert(words: &mut [u64], index: usize) {
    let (a, b) = hashes(index);
    let bits = (words.len() * 64) as u64;
    for i in 0u64..7 {
        let bit = a.wrapping_add(i.wrapping_mul(b)) % bits;
        words[bit as usize / 64] |= 1 << (bit % 64);
    }
}
pub struct GoalBloom {
    spec: CandidateSpec,
    depth: u8,
    words: Vec<u64>,
    secondary: Option<std::sync::Arc<GoalBloom>>,
}
impl GoalBloom {
    pub fn depth(&self) -> u8 {
        self.depth
    }
    pub fn secondary_depth(&self) -> Option<u8> {
        self.secondary.as_ref().map(|table| table.depth)
    }
    pub(crate) fn index_of_cubie(&self, cubie: &crate::FtoCubie) -> usize {
        self.spec.index_of_cubie(cubie)
    }
    pub(crate) fn move_index(&self, index: usize, mv: Move, tables: &TransitionTables) -> usize {
        self.spec.move_index(index, mv, tables)
    }
    pub fn heuristic(&self, coord: FtoCoord) -> u8 {
        self.heuristic_index(self.spec.index_of_coord(coord))
    }
    pub fn heuristic_cubie(&self, cubie: &crate::FtoCubie) -> u8 {
        self.heuristic_index(self.spec.index_of_cubie(cubie))
    }
    pub(crate) fn heuristic_index(&self, index: usize) -> u8 {
        let (a, b) = hashes(index);
        let bits = (self.words.len() * 64) as u64;
        let value = if (0u64..7).all(|i| {
            let bit = a.wrapping_add(i.wrapping_mul(b)) % bits;
            self.words[bit as usize / 64] & (1 << (bit % 64)) != 0
        }) {
            0
        } else {
            self.depth.saturating_add(1)
        };
        if value != 0 {
            return value;
        }
        self.secondary
            .as_ref()
            .map_or(0, |table| table.heuristic_index(index))
    }
}
pub fn discover(out: &Path) -> Vec<(String, Metadata)> {
    let Ok(entries) = fs::read_dir(out) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension()?.to_str()? != "json" {
                return None;
            }
            let meta: Metadata = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            let id = path.file_stem()?.to_str()?.to_owned();
            if id != key(&meta) || !(1..=2).contains(&meta.version) {
                return None;
            }
            Some((id, meta))
        })
        .collect()
}
static TRANSITIONS: std::sync::OnceLock<
    std::sync::Mutex<Option<std::sync::Arc<TransitionTables>>>,
> = std::sync::OnceLock::new();
pub fn set_transition_tables(tables: std::sync::Arc<TransitionTables>) {
    if let Ok(mut slot) = TRANSITIONS.get_or_init(Default::default).lock() {
        *slot = Some(tables);
    }
}
pub(crate) fn transition_tables() -> Option<std::sync::Arc<TransitionTables>> {
    TRANSITIONS
        .get_or_init(Default::default)
        .lock()
        .ok()?
        .clone()
}
type Loaded = (
    std::sync::Arc<PatternDatabases>,
    Option<std::sync::Arc<GoalBloom>>,
);
static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(String, Loaded)>>> =
    std::sync::OnceLock::new();
pub fn clear_cache() {
    if let Ok(mut tables) = TRANSITIONS.get_or_init(Default::default).lock() {
        *tables = None;
    }
    if let Ok(mut cache) = CACHE.get_or_init(Default::default).lock() {
        *cache = None;
    }
}
pub fn has_cache() -> bool {
    CACHE
        .get_or_init(Default::default)
        .try_lock()
        .ok()
        .is_some_and(|c| {
            c.as_ref()
                .is_some_and(|(_, v)| v.0.len() > 0 || v.1.is_some())
        })
}
pub fn load(
    out: &Path,
    mask: &PartialMask,
    moves: &[Move],
    cancel: Option<&AtomicBool>,
) -> Result<Loaded, String> {
    let mut candidates = discover(out);
    candidates.retain(|(_, m)| m.compatible(mask, moves));
    candidates.sort_by_key(|(_, m)| {
        std::cmp::Reverse((
            m.mask == *mask,
            m.version,
            m.entries,
            m.depth,
            m.moves == pruning::move_set_suffix(moves),
        ))
    });
    let cache_key = format!(
        "{}:{}",
        out.display(),
        candidates
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "partial cache lock poisoned")?;
    if let Some((key, loaded)) = cache.as_ref() {
        if key == &cache_key {
            return Ok(loaded.clone());
        }
    }
    *cache = None;
    let mut patterns = Vec::new();
    let mut bloom: Option<GoalBloom> = None;
    for (id, meta) in candidates {
        active(cancel)?;
        let Some(spec) = meta.spec() else { continue };
        if spec.size() != Some(meta.entries) {
            continue;
        }
        if let Some(depth) = meta.depth {
            if let Some(primary) = &bloom {
                if primary.secondary.is_some()
                    || depth > 12
                    || depth >= primary.depth
                    || spec != primary.spec
                {
                    continue;
                }
            }
            let path = out.join(format!("{id}.gbloom"));
            let Ok(size) = fs::metadata(&path).map(|m| m.len() as usize) else {
                continue;
            };
            if size <= 24 || (size - 24) % 8 != 0 {
                continue;
            }
            crate::acceleration::check_host_allocation(size as u64)?;
            use std::io::Read;
            let mut reader =
                std::io::BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?);
            let mut magic = [0u8; 16];
            if reader.read_exact(&mut magic).is_err() || &magic != b"FTO_PBLOOM_V1\0\0\0" {
                continue;
            }
            let mut checksum = [0u8; 8];
            if reader.read_exact(&mut checksum).is_err() {
                continue;
            }
            let mut words = vec![0u64; (size - 24) / 8];
            for chunk in words.chunks_mut(8192) {
                active(cancel)?;
                for word in chunk {
                    let mut b = [0u8; 8];
                    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
                    *word = u64::from_le_bytes(b);
                }
            }
            if words.iter().fold(0xcbf29ce484222325u64, |h, &w| {
                (h ^ w).wrapping_mul(0x100000001b3)
            }) != u64::from_le_bytes(checksum)
            {
                continue;
            }
            let table = GoalBloom {
                spec,
                depth,
                words,
                secondary: None,
            };
            if let Some(primary) = bloom.as_mut() {
                primary.secondary = Some(std::sync::Arc::new(table));
            } else {
                bloom = Some(table);
            }
        } else if patterns.len() < 2 {
            if let Ok(table) =
                PatternDatabase::load_cancellable(spec, out.join(format!("{id}.prun")), cancel)
            {
                patterns.push(table);
            }
        }
    }
    let loaded = (
        std::sync::Arc::new(PatternDatabases::new(patterns)),
        bloom.map(std::sync::Arc::new),
    );
    if loaded.0.len() > 0 || loaded.1.is_some() {
        *cache = Some((cache_key, loaded.clone()));
    }
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FtoCubie, moves::move_cubies};
    fn empty_mask() -> PartialMask {
        let mut m = PartialMask::all();
        m.corners.fill(false);
        m.edges.fill(false);
        m.uf_centers.fill(false);
        m.rl_centers.fill(false);
        m
    }
    #[test]
    fn entitled_slots_and_pins_tighten_center_goal_projections() {
        let mut mask = empty_mask();
        mask.uf_centers[0] = true;
        let mut colors = [1, 0, 2, 0, 1, 1, 0, 2, 2, 3, 3, 3];
        let value = coord::rank_center_colors(&colors);
        assert!(accepts(Component::UfCenter, value, &mask));
        mask.uf_entitled_slots[0] = 1;
        assert!(!accepts(Component::UfCenter, value, &mask));
        colors.swap(0, 1);
        assert!(accepts(
            Component::UfCenter,
            coord::rank_center_colors(&colors),
            &mask
        ));
        mask.uf_entitled_slots[0] = 0;
        mask.uf_center_targets[0] = Some(2);
        assert!(!accepts(
            Component::UfCenter,
            coord::rank_center_colors(&colors),
            &mask
        ));
        let plan = Plan {
            spec: CandidateSpec::new(vec![Component::UfCenter]),
            goals: vec![vec![0]],
            entries: coord::CENTER_COUNT,
            roots: 1,
        };
        let mut meta = metadata(&plan, &mask, &[Move::R], None);
        assert!(meta.compatible(&mask, &[Move::R]));
        let mut other = mask.clone();
        other.uf_center_targets[0] = Some(0);
        assert!(!meta.compatible(&other, &[Move::R]));
        meta.version = 1;
        assert!(
            meta.compatible(&other, &[Move::R]),
            "legacy tables relaxed these constraints and remain safe fallback tables"
        );
        mask.uf_center_targets[0] = None;
        mask.uf_entitled_slots[0] = 1;
        meta = metadata(&plan, &mask, &[Move::R], None);
        other = mask.clone();
        other.uf_entitled_slots[0] = 2;
        assert!(!meta.compatible(&other, &[Move::R]));
        other.uf_entitled_slots[0] = 0;
        assert!(!meta.compatible(&other, &[Move::R]));
    }

    #[test]
    fn center_projections_never_reject_a_true_entitled_identity_goal() {
        let mut checked = 0;
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let head = [a, b, c, d];
                        if (0..4).any(|i| (0..i).any(|j| head[i] == head[j])) {
                            continue;
                        }
                        let mut centers = std::array::from_fn(|i| i as u8);
                        centers[..4].copy_from_slice(&head);
                        for defined in 0..8 {
                            for entitled in 0..8_u16 {
                                let mut mask = empty_mask();
                                for p in 0..3 {
                                    mask.uf_centers[p] = defined & (1 << p) != 0;
                                }
                                mask.uf_entitled_slots[0] = entitled;
                                let count = (0..3).filter(|&p| mask.uf_centers[p]).count();
                                let home = (0..3).all(|p| {
                                    !mask.uf_centers[p] || centers[..3].contains(&(p as u8))
                                });
                                let overlap = (0..3)
                                    .filter(|&slot| {
                                        entitled & (1 << slot) != 0
                                            && centers[slot] < 3
                                            && mask.uf_centers[centers[slot] as usize]
                                    })
                                    .count();
                                let goal = home
                                    && (count == 0
                                        || count == 3
                                        || entitled == 0
                                        || entitled == 7
                                        || overlap == count.min(entitled.count_ones() as usize));
                                if !goal {
                                    continue;
                                }
                                checked += 1;
                                for component in [
                                    Component::UfCenter,
                                    Component::UfCenter2,
                                    Component::UfCenter3,
                                ] {
                                    let mut cubie = FtoCubie::solved();
                                    cubie.uf = centers;
                                    assert!(accepts(
                                        component,
                                        CandidateSpec::new(vec![component])
                                            .index_of_coord(cubie.coord())
                                            as u32,
                                        &mask
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 100);
    }

    #[test]
    fn partial_goal_relaxes_unknown_pieces_and_fast_indexes_match_full_coordinates() {
        let mut mask = PartialMask::all();
        mask.corners[0] = false;
        mask.corners[1] = false;
        mask.corners[2] = false;
        mask.edges[0] = false;
        mask.edges[1] = false;
        mask.edges[2] = false;
        let mut goal = FtoCubie::solved();
        goal.cp = [1, 2, 0, 3, 4, 5];
        goal.ep[..3].copy_from_slice(&[1, 2, 0]);
        let components = [
            Component::Corner,
            Component::E0,
            Component::E1,
            Component::E2,
            Component::E3,
            Component::Edge3,
            Component::Edge4,
            Component::UfCenter,
            Component::RlCenter,
            Component::UfCenter2,
            Component::UfCenter3,
            Component::RlCenter2,
            Component::RlCenter3,
        ];
        for component in components {
            let spec = CandidateSpec::new(vec![component]);
            assert!(accepts(
                component,
                component.value_of(FtoCoord::from_cubie(&goal)),
                &mask
            ));
            assert_eq!(
                spec.index_of_cubie(&goal),
                spec.index_of_coord(FtoCoord::from_cubie(&goal))
            );
        }
        let moves = move_cubies();
        let mut state = goal;
        for mv in Move::ALL.into_iter().cycle().take(100) {
            state = state.compose(&moves[mv.idx()]);
            for component in components {
                let spec = CandidateSpec::new(vec![component]);
                assert_eq!(
                    spec.index_of_cubie(&state),
                    spec.index_of_coord(FtoCoord::from_cubie(&state)),
                    "{}",
                    component.name()
                );
            }
        }
    }
    #[test]
    fn compatibility_rejects_weaker_goals_other_modes_and_extra_moves() {
        let mut cared = empty_mask();
        cared.edges[0] = true;
        let plan = Plan {
            spec: CandidateSpec::new(vec![Component::E0]),
            goals: vec![vec![0]],
            entries: 924,
            roots: 1,
        };
        let meta = metadata(&plan, &cared, &[Move::R], None);
        assert!(meta.compatible(&PartialMask::all(), &[Move::R]));
        assert!(!meta.compatible(&empty_mask(), &[Move::R]));
        assert!(!meta.compatible(&cared, &[Move::Rp]));
        cared.last_layer_centers = true;
        assert!(!meta.compatible(&cared, &[Move::R]));
    }
    #[test]
    #[ignore = "uses the existing transition cache"]
    fn dedicated_partial_tables_reverse_directed_moves_and_bloom_has_no_false_negatives() {
        let tables = TransitionTables::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../cache/transition-table.tran"
        ))
        .unwrap();
        let moves = move_cubies();
        let turned = FtoCubie::solved().compose(&moves[Move::R.idx()]);
        let affected = (0..6)
            .find(|&i| turned.cp[i] as usize != i || turned.co[i] != 0)
            .unwrap();
        let mut mask = empty_mask();
        mask.corners[affected] = true;
        let goals: Vec<_> = (0..coord::CORNER_COUNT as u32)
            .filter(|&v| accepts(Component::Corner, v, &mask))
            .collect();
        let plan = Plan {
            spec: CandidateSpec::new(vec![Component::Corner]),
            entries: coord::CORNER_COUNT,
            roots: goals.len(),
            goals: vec![goals],
        };
        let out = std::env::temp_dir().join(format!("fto-goal-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&out);
        // Mixed-radix goal lists seed the full Cartesian product, including
        // correlated projections relaxed independently at the goal.
        let joint = CandidateSpec::new(vec![Component::Corner, Component::E0]);
        let c = FtoCoord::solved();
        let r = FtoCoord::from_cubie(&turned);
        let mut first = vec![u32::from(c.corner), u32::from(r.corner)];
        first.sort_unstable();
        first.dedup();
        let mut second = vec![u32::from(c.edge.e0), u32::from(r.edge.e0)];
        second.sort_unstable();
        second.dedup();
        let goals = vec![first, second];
        let (stats, distances) = pruning::build_pruning_table_from_goals(
            &joint,
            &tables,
            joint.size().unwrap(),
            0,
            &goals,
            &[Move::R],
            2,
            None,
            None,
            Some(1),
            true,
        )
        .unwrap();
        assert_eq!(stats.histogram[0], goals[0].len() * goals[1].len());
        for &a in &goals[0] {
            for &b in &goals[1] {
                let index = a as usize * coord::EDGE_CHOICE_COUNT + b as usize;
                assert_eq!(distances[index], 0);
                assert!(distances[joint.move_index(index, Move::Rp, &tables)] <= 1);
            }
        }
        drop(distances);
        let mut walked = FtoCubie::solved();
        let mut index = joint.index_of_cubie(&walked);
        for m in Move::ALL.into_iter().cycle().take(100) {
            walked = walked.compose(&moves[m.idx()]);
            index = joint.move_index(index, m, &tables);
            assert_eq!(index, joint.index_of_cubie(&walked));
        }
        let meta = metadata(&plan, &mask, &[Move::R], None);
        generate(&plan, &meta, &tables, &out, 2, None, None, 0).unwrap();
        let (pdb, _) = load(&out, &mask, &[Move::R], None).unwrap();
        assert_eq!(
            pdb.heuristic_cubie(&turned),
            2,
            "R needs two R turns to undo a defined affected corner"
        );
        let cached = load(&out, &mask, &[Move::R], None).unwrap();
        assert!(std::sync::Arc::ptr_eq(&pdb, &cached.0));
        let meta = metadata(&plan, &mask, &[Move::R], Some(1));
        generate(&plan, &meta, &tables, &out, 2, None, None, 1).unwrap();
        let (_, bloom) = load(&out, &mask, &[Move::R], None).unwrap();
        let bloom = bloom.unwrap();
        assert_eq!(bloom.heuristic_cubie(&FtoCubie::solved()), 0);
        assert_eq!(
            bloom.heuristic_cubie(&FtoCubie::solved().compose(&moves[Move::Rp.idx()])),
            0
        );
        assert_eq!(bloom.heuristic_cubie(&turned), 2);
        // A corrupt Bloom payload must be rejected, never used to prune solutions.
        let path = out.join(format!("{}.gbloom", key(&meta)));
        let mut bytes = fs::read(&path).unwrap();
        bytes[32] ^= 1;
        fs::write(path, bytes).unwrap();
        clear_cache();
        assert!(load(&out, &mask, &[Move::R], None).unwrap().1.is_none());
        let mut almost_full = PartialMask::all();
        almost_full.uf_centers[0] = false;
        let large = plan_for_test(&almost_full, &tables);
        println!(
            "Default partial joint plan: {} = {:.1} MiB ({} goal projections)",
            large.spec.name(),
            large.entries as f64 / 1048576.,
            large.roots
        );
        assert!(large.entries >= 128 * 1024 * 1024 && large.entries <= 512 * 1024 * 1024);
        clear_cache();
        fs::remove_dir_all(out).unwrap();
    }
    fn plan_for_test(mask: &PartialMask, tables: &TransitionTables) -> Plan {
        plan(mask, tables, 512 * 1024 * 1024, None).unwrap()
    }
}
