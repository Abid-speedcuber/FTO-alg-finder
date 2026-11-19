use std::{
    fs::File,
    io::{self, BufWriter, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    thread,
    time::Instant,
};

use crate::{
    FtoCoord,
    coord::{CENTER_COUNT, CENTER3_COUNT, CORNER_COUNT, EDGE_CHOICE_COUNT},
    moves::{MOVE_COUNT, Move},
    tables::TransitionTables,
};

mod subgroup;
use subgroup::CompactIndexing;

const UNVISITED: u8 = u8::MAX;
const PRUN_MAGIC: &[u8; 16] = b"FTO_PRUN_V1\0\0\0\0\0";

pub const CANCELLED: &str = "solve cancelled";

#[derive(Clone, Debug)]
pub struct PruningProgress {
    pub name: String,
    pub depth: usize,
    pub expanded: usize,
    pub reached: usize,
    pub total: usize,
}

pub type PruningReporter<'a> = dyn Fn(PruningProgress) + Send + Sync + 'a;

#[derive(Clone, Debug)]
pub struct PatternDatabase {
    spec: CandidateSpec,
    table: Vec<u8>,
}

impl PatternDatabase {
    pub fn load_or_build(
        spec: CandidateSpec,
        tables: &TransitionTables,
        max_entries: usize,
        progress_interval: usize,
        path: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let path = path.as_ref();
        match read_table(path, spec.size().unwrap_or(0)) {
            Ok(table) => Ok(Self { spec, table }),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Err(error.to_string()),
            Err(_) => {
                let (_, table) =
                    build_pruning_table(&spec, tables, max_entries, progress_interval)?;
                write_table(path, &table).map_err(|error| error.to_string())?;
                Ok(Self { spec, table })
            }
        }
    }

    pub fn load(spec: CandidateSpec, path: impl AsRef<Path>) -> Result<Self, String> {
        let table = read_table(path.as_ref(), spec.size().unwrap_or(0))
            .map_err(|error| error.to_string())?;
        Ok(Self { spec, table })
    }

    pub fn load_or_build_with_moves_reporting(
        spec: CandidateSpec,
        tables: &TransitionTables,
        progress_interval: usize,
        path: impl AsRef<Path>,
        moves: &[Move],
        threads: usize,
        cancel: Option<&AtomicBool>,
        report: Option<&PruningReporter<'_>>,
    ) -> Result<Self, String> {
        let path = path.as_ref();
        match if path.exists() {
            read_table_reporting(
                path,
                spec.size().unwrap_or(0),
                report,
                &format!("loading {}", spec.name()),
                cancel,
            )
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "pruning table does not exist",
            ))
        } {
            Ok(table) => Ok(Self { spec, table }),
            Err(_) => {
                let (_, table) = build_pruning_table_with_moves_threaded(
                    &spec,
                    tables,
                    usize::MAX,
                    progress_interval,
                    moves,
                    threads,
                    cancel,
                    report,
                )?;
                write_table_with_metadata_reporting(
                    path,
                    &table,
                    &format!("spec={}\nmoves={}\n", spec.name(), move_set_suffix(moves)),
                    report,
                    &format!("saving {}", spec.name()),
                    cancel,
                )
                .map_err(|error| error.to_string())?;
                Ok(Self { spec, table })
            }
        }
    }

    pub fn load_cancellable(
        spec: CandidateSpec,
        path: impl AsRef<Path>,
        cancel: Option<&AtomicBool>,
    ) -> Result<Self, String> {
        crate::acceleration::check_host_allocation(spec.size().unwrap_or(0) as u64)?;
        let table = read_table_reporting(path, spec.size().unwrap_or(0), None, "", cancel)
            .map_err(|e| e.to_string())?;
        Ok(Self { spec, table })
    }

    #[must_use]
    pub fn name(&self) -> String {
        self.spec.name()
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        self.table.len()
    }

    #[must_use]
    pub fn value(&self, coord: FtoCoord) -> u8 {
        let value = self.table[self.spec.index_of_coord(coord)];
        if value == UNVISITED { UNVISITED } else { value }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PatternDatabases {
    tables: Vec<PatternDatabase>,
}

impl PatternDatabases {
    #[must_use]
    pub fn new(tables: Vec<PatternDatabase>) -> Self {
        Self { tables }
    }

    #[must_use]
    pub fn heuristic(&self, coord: FtoCoord) -> u8 {
        self.tables
            .iter()
            .map(|table| table.value(coord))
            .max()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn heuristic_cubie(&self, cubie: &crate::FtoCubie) -> u8 {
        self.tables
            .iter()
            .map(|table| table.table[table.spec.index_of_cubie(cubie)])
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn indices_of_cubie(&self, cubie: &crate::FtoCubie) -> Vec<usize> {
        self.tables
            .iter()
            .map(|t| t.spec.index_of_cubie(cubie))
            .collect()
    }
    pub(crate) fn heuristic_indices(&self, indices: &[usize]) -> u8 {
        self.tables
            .iter()
            .zip(indices)
            .map(|(t, &i)| t.table[i])
            .max()
            .unwrap_or(0)
    }
    pub(crate) fn move_indices(
        &self,
        indices: &[usize],
        mv: Move,
        tables: &TransitionTables,
    ) -> Vec<usize> {
        self.tables
            .iter()
            .zip(indices)
            .map(|(t, &i)| t.spec.move_index(i, mv, tables))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.tables.iter().map(PatternDatabase::bytes).sum()
    }

    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.tables.iter().map(PatternDatabase::name).collect()
    }
}

#[derive(Clone, Debug)]
pub struct SolverPruning {
    edge3_uf3: Vec<u8>,
    corner_uf3: Vec<u8>,
    compact: Option<CompactIndexing>,
    orbit_swap: Option<crate::symmetry::OrbitSwap>,
}

impl SolverPruning {
    pub fn load_or_build(
        tables: &TransitionTables,
        out_dir: &Path,
        progress_interval: usize,
    ) -> Result<Self, String> {
        Self::load_or_build_with_moves(tables, out_dir, progress_interval, &Move::ALL)
    }

    pub fn load_or_build_with_moves(
        tables: &TransitionTables,
        out_dir: &Path,
        progress_interval: usize,
        moves: &[Move],
    ) -> Result<Self, String> {
        Self::load_or_build_with_moves_threaded(tables, out_dir, progress_interval, moves, 1)
    }

    pub fn load_or_build_with_moves_threaded(
        tables: &TransitionTables,
        out_dir: &Path,
        progress_interval: usize,
        moves: &[Move],
        threads: usize,
    ) -> Result<Self, String> {
        Self::load_or_build_with_moves_reporting(
            tables,
            out_dir,
            progress_interval,
            moves,
            threads,
            None,
            None,
        )
    }

    pub fn load_or_build_with_moves_reporting(
        tables: &TransitionTables,
        out_dir: &Path,
        progress_interval: usize,
        moves: &[Move],
        threads: usize,
        cancel: Option<&AtomicBool>,
        report: Option<&PruningReporter<'_>>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
        let suffix = move_set_suffix(moves);
        let file_suffix = move_set_codename(&suffix);
        let edge_path = out_dir.join(format!("e3cf3_{file_suffix}.prun"));
        let corner_path = out_dir.join(format!("ccf3_{file_suffix}.prun"));
        if let Some(compact) = CompactIndexing::build(tables, moves, FtoCoord::solved()) {
            let edge3_uf3 = compact.load_or_build(
                subgroup::Kind::Edge,
                &edge_path,
                progress_interval,
                cancel,
                report,
            )?;
            let corner_uf3 = compact.load_or_build(
                subgroup::Kind::Corner,
                &corner_path,
                progress_interval,
                cancel,
                report,
            )?;
            return Ok(Self {
                edge3_uf3,
                corner_uf3,
                compact: Some(compact),
                orbit_swap: crate::symmetry::OrbitSwap::for_moves(moves),
            });
        }
        let edge3_uf3 = load_or_build_solver_table(
            CandidateSpec::new(vec![Component::Edge3, Component::UfCenter3]),
            tables,
            progress_interval,
            edge_path,
            moves,
            threads,
            cancel,
            report,
        )?;
        let corner_uf3 = load_or_build_solver_table(
            CandidateSpec::new(vec![Component::Corner, Component::UfCenter3]),
            tables,
            progress_interval,
            corner_path,
            moves,
            threads,
            cancel,
            report,
        )?;
        Ok(Self {
            edge3_uf3,
            corner_uf3,
            compact: None,
            orbit_swap: crate::symmetry::OrbitSwap::for_moves(moves),
        })
    }

    pub fn build_from_coord(
        root: FtoCoord,
        tables: &TransitionTables,
        progress_interval: usize,
    ) -> Result<Self, String> {
        Self::build_from_coord_with_moves(root, tables, progress_interval, &Move::ALL)
    }

    pub fn build_from_coord_with_moves(
        root: FtoCoord,
        tables: &TransitionTables,
        progress_interval: usize,
        moves: &[Move],
    ) -> Result<Self, String> {
        Self::build_from_coord_with_moves_threaded(root, tables, progress_interval, moves, 1)
    }

    pub fn build_from_coord_with_moves_threaded(
        root: FtoCoord,
        tables: &TransitionTables,
        progress_interval: usize,
        moves: &[Move],
        threads: usize,
    ) -> Result<Self, String> {
        if let Some(compact) = CompactIndexing::build(tables, moves, root) {
            let edge3_uf3 =
                compact.build_table(subgroup::Kind::Edge, progress_interval, None, None)?;
            let corner_uf3 =
                compact.build_table(subgroup::Kind::Corner, progress_interval, None, None)?;
            return Ok(Self {
                edge3_uf3,
                corner_uf3,
                compact: Some(compact),
                orbit_swap: (root == FtoCoord::solved())
                    .then(|| crate::symmetry::OrbitSwap::for_moves(moves))
                    .flatten(),
            });
        }
        let edge3_uf3 = build_solver_table_from_root(
            CandidateSpec::new(vec![Component::Edge3, Component::UfCenter3]),
            root,
            tables,
            progress_interval,
            moves,
            threads,
        )?;
        let corner_uf3 = build_solver_table_from_root(
            CandidateSpec::new(vec![Component::Corner, Component::UfCenter3]),
            root,
            tables,
            progress_interval,
            moves,
            threads,
        )?;
        Ok(Self {
            edge3_uf3,
            corner_uf3,
            compact: None,
            orbit_swap: (root == FtoCoord::solved())
                .then(|| crate::symmetry::OrbitSwap::for_moves(moves))
                .flatten(),
        })
    }

    #[inline]
    #[must_use]
    pub fn heuristic(&self, edge3_uf3_idx: usize, corner_uf3_idx: usize) -> u8 {
        if let Some(compact) = &self.compact {
            return compact
                .value(subgroup::Kind::Edge, &self.edge3_uf3, edge3_uf3_idx)
                .max(compact.value(subgroup::Kind::Corner, &self.corner_uf3, corner_uf3_idx));
        }
        live_depth(self.edge3_uf3[edge3_uf3_idx]).max(live_depth(self.corner_uf3[corner_uf3_idx]))
    }

    /// Reads indices already carried in the search state's coordinate system.
    #[inline]
    pub(crate) fn heuristic_cached(&self, edge: usize, corner: usize) -> u8 {
        self.edge3_uf3[edge].max(self.corner_uf3[corner])
    }

    /// Stop dense-table reads once the branch is rejected. Compact tables
    /// retain their unconditional maximum because both reads are inexpensive.
    #[inline]
    pub(crate) fn heuristic_cached_bounded(&self, edge: usize, corner: usize, bound: u8) -> u8 {
        let first = self.edge3_uf3[edge];
        if first > bound {
            first
        } else {
            first.max(self.corner_uf3[corner])
        }
    }

    /// Select the dense-table loop once, outside the recursive search.
    pub(crate) fn uses_bounded_reads(&self) -> bool {
        self.compact.is_none()
    }

    /// Select extra coordinate work once; ordinary dense searches keep their original path.
    pub(crate) fn for_moves(&self) -> Option<&Self> {
        (self.compact.is_some() || self.orbit_swap.is_some()).then_some(self)
    }

    #[inline]
    pub(crate) fn has_orbit_swap(&self) -> bool {
        self.orbit_swap.is_some()
    }

    pub(crate) fn swapped_root(&self, coord: FtoCoord) -> crate::symmetry::SwappedPruning {
        let Some(swap) = &self.orbit_swap else {
            return crate::symmetry::SwappedPruning::default();
        };
        let mut projected = crate::symmetry::OrbitSwap::project(coord);
        projected.bound = self.swapped_bound(swap.corner(coord.corner), projected);
        projected
    }

    #[inline]
    pub(crate) fn swapped_step_bounded(
        &self,
        tables: &TransitionTables,
        parent: crate::symmetry::SwappedPruning,
        corner: u16,
        mv: Move,
        bound: u8,
    ) -> crate::symmetry::SwappedPruning {
        let Some(swap) = &self.orbit_swap else {
            return crate::symmetry::SwappedPruning::default();
        };
        let mapped = crate::symmetry::OrbitSwap::map_move(mv)
            .expect("orbit-swap pruning requires a symmetry-closed move set");
        let mut child = crate::symmetry::SwappedPruning {
            edge3: tables.edge3_move(parent.edge3, mapped),
            uf3: tables.uf_center3_move(parent.uf3, mapped),
            bound: 0,
        };
        child.bound = self.swapped_bound_bounded(swap.corner(corner), child, bound);
        child
    }

    #[inline]
    fn swapped_bound(&self, corner: u16, projected: crate::symmetry::SwappedPruning) -> u8 {
        self.swapped_bound_bounded(corner, projected, u8::MAX)
    }

    #[inline]
    fn swapped_bound_bounded(
        &self,
        corner: u16,
        projected: crate::symmetry::SwappedPruning,
        bound: u8,
    ) -> u8 {
        let (edge_index, corner_index) = self
            .cache_indices(corner, projected.edge3, projected.uf3, false)
            .unwrap_or_else(|| {
                (
                    Self::edge3_uf3_index(projected.edge3, projected.uf3),
                    Self::corner_uf3_index(corner, projected.uf3),
                )
            });
        self.heuristic_cached_bounded(edge_index, corner_index, bound)
    }

    #[inline(always)]
    pub(crate) fn compact_step(
        &self,
        tables: &TransitionTables,
        corner: u16,
        edge: u16,
        uf: u16,
        mv: Move,
    ) -> Option<subgroup::CompactChild> {
        let compact = self.compact.as_ref()?;
        if let Some(child) = compact.step(corner, edge, uf, mv) {
            return Some(child);
        }
        let corner = tables.corner_move(corner, mv);
        let edge3 = tables.edge3_move(edge, mv);
        let uf3 = tables.uf_center3_move(uf, mv);
        let (edge_index, corner_index) = compact.cache_indices(corner, edge3, uf3, false);
        Some(subgroup::CompactChild {
            corner,
            edge3,
            uf3,
            edge_index,
            corner_index,
        })
    }

    #[inline]
    pub(crate) fn cache_indices(
        &self,
        corner: u16,
        edge: u16,
        uf: u16,
        reject_outside: bool,
    ) -> Option<(usize, usize)> {
        Some(
            self.compact
                .as_ref()?
                .cache_indices(corner, edge, uf, reject_outside),
        )
    }

    #[cfg(test)]
    pub(crate) fn without_swap_for_tests(&self) -> Self {
        let mut copy = self.clone();
        copy.orbit_swap = None;
        copy
    }

    #[cfg(test)]
    pub(crate) fn dense_reference_for_tests(&self) -> Self {
        let compact = self.compact.as_ref().expect("needs a compact source");
        Self {
            edge3_uf3: compact.expand_for_tests(subgroup::Kind::Edge, &self.edge3_uf3),
            corner_uf3: compact.expand_for_tests(subgroup::Kind::Corner, &self.corner_uf3),
            compact: None,
            orbit_swap: self.orbit_swap.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn verify_distances_for_tests(&self, tables: &TransitionTables) {
        let compact = self.compact.as_ref().expect("needs compact storage");
        compact.verify_distances_for_tests(subgroup::Kind::Edge, &self.edge3_uf3, tables);
        compact.verify_distances_for_tests(subgroup::Kind::Corner, &self.corner_uf3, tables);
    }

    #[must_use]
    pub fn heuristic_for_coord(&self, coord: FtoCoord) -> u8 {
        self.heuristic(
            Self::edge3_uf3_index(coord.edge3, coord.uf_center3),
            Self::corner_uf3_index(coord.corner, coord.uf_center3),
        )
        .max(self.swapped_root(coord).bound)
    }

    #[must_use]
    pub fn edge3_uf3_index(edge3: u16, uf3: u16) -> usize {
        usize::from(edge3) * CENTER3_COUNT + usize::from(uf3)
    }

    #[must_use]
    pub fn corner_uf3_index(corner: u16, uf3: u16) -> usize {
        usize::from(corner) * CENTER3_COUNT + usize::from(uf3)
    }

    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.edge3_uf3.len()
            + self.corner_uf3.len()
            + self.compact.as_ref().map_or(0, CompactIndexing::bytes)
            + self
                .orbit_swap
                .as_ref()
                .map_or(0, crate::symmetry::OrbitSwap::bytes)
    }
}

const fn live_depth(value: u8) -> u8 {
    if value == UNVISITED { UNVISITED } else { value }
}

fn load_or_build_solver_table(
    spec: CandidateSpec,
    tables: &TransitionTables,
    progress_interval: usize,
    path: impl AsRef<Path>,
    moves: &[Move],
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<Vec<u8>, String> {
    let path = path.as_ref();
    eprintln!(
        "loading pruning table {} ({})",
        spec.name(),
        crate::format_bytes(spec.size().unwrap_or(0) as u64)
    );
    match if path.exists() {
        read_table_reporting(
            path,
            spec.size().unwrap_or(0),
            report,
            &format!("loading {}", spec.name()),
            cancel,
        )
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "pruning table does not exist",
        ))
    } {
        Ok(table) => Ok(table),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Err(error.to_string()),
        Err(_) => {
            let (_, table) = build_pruning_table_with_moves_threaded(
                &spec,
                tables,
                usize::MAX,
                progress_interval,
                moves,
                threads,
                cancel,
                report,
            )?;
            write_table_with_metadata_reporting(
                path,
                &table,
                &format!("spec={}\nmoves={}\n", spec.name(), move_set_suffix(moves)),
                report,
                &format!("saving {}", spec.name()),
                cancel,
            )
            .map_err(|error| error.to_string())?;
            Ok(table)
        }
    }
}

fn report_table_io(report: Option<&PruningReporter<'_>>, name: &str, reached: usize, total: usize) {
    if let Some(report) = report {
        report(PruningProgress {
            name: name.to_owned(),
            depth: 0,
            expanded: 0,
            reached,
            total: total.max(1),
        });
    }
}

fn build_solver_table_from_root(
    spec: CandidateSpec,
    root: FtoCoord,
    tables: &TransitionTables,
    progress_interval: usize,
    moves: &[Move],
    threads: usize,
) -> Result<Vec<u8>, String> {
    let root_index = spec.index_of_coord(root);
    let (_, table) = build_pruning_table_from_index_threaded(
        &spec,
        tables,
        usize::MAX,
        progress_interval,
        root_index,
        moves,
        threads,
        None,
        None,
    )?;
    Ok(table)
}

pub fn move_set_suffix(moves: &[Move]) -> String {
    if moves == Move::ALL.as_slice() {
        return "all-v2".to_owned();
    }
    let mut sorted = moves.to_vec();
    sorted.sort_unstable_by_key(|mv| mv.idx());
    sorted
        .iter()
        .map(|mv| format!("{mv:?}"))
        .collect::<Vec<_>>()
        .join("_")
}

pub fn move_set_codename(suffix: &str) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    suffix.hash(&mut hasher);
    let value = hasher.finish();
    format!("{:04x}", value as u16)
}

pub fn parse_move_set_suffix(suffix: &str) -> Option<Vec<Move>> {
    if suffix == "all-v2" {
        return Some(Move::ALL.to_vec());
    }
    // The original all-moves cache contains only the first 38 moves.
    if suffix == "all" {
        return Some(Move::ALL[..38].to_vec());
    }
    let mut moves = Vec::with_capacity(MOVE_COUNT);
    for token in suffix.split('_') {
        let mv = Move::ALL
            .iter()
            .copied()
            .find(|mv| format!("{mv:?}") == token)?;
        if moves.contains(&mv) {
            return None;
        }
        moves.push(mv);
    }
    if moves.is_empty() {
        return None;
    }
    Some(moves)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Component {
    Corner,
    E0,
    E1,
    E2,
    E3,
    Edge3,
    Edge4,
    UfCenter,
    RlCenter,
    UfCenter2,
    UfCenter3,
    RlCenter2,
    RlCenter3,
    LlUfCenterA,
    LlUfCenterB,
}

impl Component {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Corner => "corner",
            Self::E0 => "e0",
            Self::E1 => "e1",
            Self::E2 => "e2",
            Self::E3 => "e3",
            Self::Edge3 => "edge3",
            Self::Edge4 => "edge4",
            Self::UfCenter => "uf",
            Self::RlCenter => "rl",
            Self::UfCenter2 => "uf2",
            Self::UfCenter3 => "uf3",
            Self::RlCenter2 => "rl2",
            Self::RlCenter3 => "rl3",
            Self::LlUfCenterA => "llufA",
            Self::LlUfCenterB => "llufB",
        }
    }

    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::Corner => CORNER_COUNT,
            Self::E0 | Self::E1 | Self::E2 | Self::E3 => EDGE_CHOICE_COUNT,
            Self::Edge3 => crate::coord::EDGE3_COUNT,
            Self::Edge4 => crate::coord::EDGE4_COUNT,
            Self::UfCenter | Self::RlCenter => CENTER_COUNT,
            Self::UfCenter2 | Self::RlCenter2 => crate::coord::CENTER2_COUNT,
            Self::UfCenter3 | Self::RlCenter3 | Self::LlUfCenterA | Self::LlUfCenterB => {
                crate::coord::CENTER3_COUNT
            }
        }
    }

    #[must_use]
    pub fn value_of(self, coord: FtoCoord) -> u32 {
        match self {
            Self::Corner => u32::from(coord.corner),
            Self::E0 => u32::from(coord.edge.e0),
            Self::E1 => u32::from(coord.edge.e1),
            Self::E2 => u32::from(coord.edge.e2),
            Self::E3 => u32::from(coord.edge.e3),
            Self::Edge3 => u32::from(coord.edge3),
            Self::Edge4 => coord.edge4,
            Self::UfCenter => coord.uf_center,
            Self::RlCenter => coord.rl_center,
            Self::UfCenter2 => u32::from(coord.uf_center2),
            Self::UfCenter3 => u32::from(coord.uf_center3),
            Self::RlCenter2 => u32::from(coord.rl_center2),
            Self::RlCenter3 => u32::from(coord.rl_center3),
            Self::LlUfCenterA => u32::from(coord.ll_uf_center_a),
            Self::LlUfCenterB => u32::from(coord.ll_uf_center_b),
        }
    }

    #[must_use]
    pub fn solved_value(self) -> u32 {
        let solved = FtoCoord::solved();
        match self {
            Self::Corner => u32::from(solved.corner),
            Self::E0 => u32::from(solved.edge.e0),
            Self::E1 => u32::from(solved.edge.e1),
            Self::E2 => u32::from(solved.edge.e2),
            Self::E3 => u32::from(solved.edge.e3),
            Self::Edge3 => u32::from(solved.edge3),
            Self::Edge4 => solved.edge4,
            Self::UfCenter => solved.uf_center,
            Self::RlCenter => solved.rl_center,
            Self::UfCenter2 => u32::from(solved.uf_center2),
            Self::UfCenter3 => u32::from(solved.uf_center3),
            Self::RlCenter2 => u32::from(solved.rl_center2),
            Self::RlCenter3 => u32::from(solved.rl_center3),
            Self::LlUfCenterA => u32::from(solved.ll_uf_center_a),
            Self::LlUfCenterB => u32::from(solved.ll_uf_center_b),
        }
    }

    #[must_use]
    pub fn next(self, value: u32, mv: Move, tables: &TransitionTables) -> u32 {
        match self {
            Self::Corner => u32::from(tables.corner_move(value as u16, mv)),
            Self::E0 | Self::E1 | Self::E2 | Self::E3 => {
                u32::from(tables.edge_choice_move(value as u16, mv))
            }
            Self::Edge3 => u32::from(tables.edge3_move(value as u16, mv)),
            Self::Edge4 => tables.edge4_move(value, mv),
            Self::UfCenter => tables.uf_center_move(value, mv),
            Self::RlCenter => tables.rl_center_move(value, mv),
            Self::UfCenter2 => u32::from(tables.uf_center2_move(value as u16, mv)),
            Self::UfCenter3 => u32::from(tables.uf_center3_move(value as u16, mv)),
            Self::RlCenter2 => u32::from(tables.rl_center2_move(value as u16, mv)),
            Self::RlCenter3 => u32::from(tables.rl_center3_move(value as u16, mv)),
            Self::LlUfCenterA | Self::LlUfCenterB => {
                u32::from(tables.uf_center3_move(value as u16, mv))
            }
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "corner" | "c" => Some(Self::Corner),
            "e0" => Some(Self::E0),
            "e1" => Some(Self::E1),
            "e2" => Some(Self::E2),
            "e3" => Some(Self::E3),
            "edge3" => Some(Self::Edge3),
            "edge4" => Some(Self::Edge4),
            "uf" | "uf_center" => Some(Self::UfCenter),
            "rl" | "rl_center" => Some(Self::RlCenter),
            "uf2" | "uf_center2" => Some(Self::UfCenter2),
            "uf3" | "uf_center3" => Some(Self::UfCenter3),
            "rl2" | "rl_center2" => Some(Self::RlCenter2),
            "rl3" | "rl_center3" => Some(Self::RlCenter3),
            "llufA" | "ll_uf_a" | "ll_uf_center_a" => Some(Self::LlUfCenterA),
            "llufB" | "ll_uf_b" | "ll_uf_center_b" => Some(Self::LlUfCenterB),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateSpec {
    pub components: Vec<Component>,
}

impl CandidateSpec {
    #[must_use]
    pub fn new(components: Vec<Component>) -> Self {
        assert!(!components.is_empty());
        Self { components }
    }

    #[must_use]
    pub fn name(&self) -> String {
        self.components
            .iter()
            .map(|component| component.name())
            .collect::<Vec<_>>()
            .join("+")
    }

    #[must_use]
    pub fn size(&self) -> Option<usize> {
        self.components
            .iter()
            .try_fold(1_usize, |acc, component| acc.checked_mul(component.size()))
    }

    #[must_use]
    pub fn solved_index(&self) -> usize {
        self.pack(
            &self
                .components
                .iter()
                .map(|component| component.solved_value())
                .collect::<Vec<_>>(),
        )
    }

    #[must_use]
    pub fn index_of_coord(&self, coord: FtoCoord) -> usize {
        self.pack(
            &self
                .components
                .iter()
                .map(|component| component.value_of(coord))
                .collect::<Vec<_>>(),
        )
    }

    pub fn index_of_cubie(&self, cubie: &crate::FtoCubie) -> usize {
        self.components.iter().fold(0, |index, &component| {
            use crate::coord;
            use Component::*;
            let value = match component {
                Corner => u32::from(coord::rank_corner(&cubie.cp, &cubie.co)),
                E0 | E1 | E2 | E3 => {
                    let e = coord::EdgeCoord::from_ep(&cubie.ep);
                    u32::from(match component {
                        E0 => e.e0,
                        E1 => e.e1,
                        E2 => e.e2,
                        _ => e.e3,
                    })
                }
                Edge3 => {
                    coord::rank_multiset_colors(&cubie.ep.map(|p| p / 4), &coord::EDGE3_COUNTS)
                }
                Edge4 => {
                    coord::rank_multiset_colors(&cubie.ep.map(|p| p / 3), &coord::EDGE4_COUNTS)
                }
                UfCenter | RlCenter => coord::rank_center_colors(
                    &if component == UfCenter {
                        cubie.uf
                    } else {
                        cubie.rl
                    }
                    .map(|p| p / 3),
                ),
                UfCenter2 | RlCenter2 => coord::rank_multiset_colors(
                    &if component == UfCenter2 {
                        cubie.uf
                    } else {
                        cubie.rl
                    }
                    .map(|p| p / 6),
                    &coord::CENTER2_COUNTS,
                ),
                UfCenter3 | RlCenter3 => coord::rank_multiset_colors(
                    &if component == UfCenter3 {
                        cubie.uf
                    } else {
                        cubie.rl
                    }
                    .map(|p| {
                        if p < 3 {
                            0
                        } else if p < 6 {
                            1
                        } else {
                            2
                        }
                    }),
                    &coord::CENTER3_COUNTS,
                ),
                _ => component.value_of(FtoCoord::from_cubie(cubie)),
            };
            index * component.size() + value as usize
        })
    }

    pub(crate) fn move_index(&self, index: usize, mv: Move, tables: &TransitionTables) -> usize {
        let n = self.components.len();
        if n <= 13 {
            let mut values = [0u32; 13];
            let mut next = [0u32; 13];
            self.next_index(index, mv, tables, &mut values[..n], &mut next[..n])
        } else {
            self.next_index(index, mv, tables, &mut vec![0; n], &mut vec![0; n])
        }
    }

    fn pack(&self, values: &[u32]) -> usize {
        let mut idx = 0_usize;
        for (&value, component) in values.iter().zip(&self.components) {
            idx = idx * component.size() + value as usize;
        }
        idx
    }

    fn unpack(&self, mut idx: usize, values: &mut [u32]) {
        for i in (0..self.components.len()).rev() {
            let size = self.components[i].size();
            values[i] = (idx % size) as u32;
            idx /= size;
        }
    }

    fn next_index(
        &self,
        idx: usize,
        mv: Move,
        tables: &TransitionTables,
        values: &mut [u32],
        next_values: &mut [u32],
    ) -> usize {
        self.unpack(idx, values);
        for (i, component) in self.components.iter().copied().enumerate() {
            next_values[i] = component.next(values[i], mv, tables);
        }
        self.pack(next_values)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PruningStats {
    pub name: String,
    pub table_entries: usize,
    pub table_bytes: usize,
    pub reached: usize,
    pub max_depth: u8,
    pub average_depth_milli: u32,
    pub histogram: Vec<usize>,
}

pub fn evaluate_candidate(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
) -> Result<PruningStats, String> {
    evaluate_candidate_with_progress(spec, tables, max_entries, 0)
}

pub fn evaluate_candidate_with_progress(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
) -> Result<PruningStats, String> {
    let (stats, _) = build_pruning_table(spec, tables, max_entries, progress_interval)?;
    Ok(stats)
}

pub fn build_and_save_candidate(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    path: impl AsRef<Path>,
) -> Result<PruningStats, String> {
    let (stats, table) = build_pruning_table(spec, tables, max_entries, progress_interval)?;
    write_table(path, &table).map_err(|error| error.to_string())?;
    Ok(stats)
}

fn build_pruning_table(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
) -> Result<(PruningStats, Vec<u8>), String> {
    build_pruning_table_with_moves(
        spec,
        tables,
        max_entries,
        progress_interval,
        &Move::ALL,
        None,
        None,
    )
}

fn build_pruning_table_with_moves(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    moves: &[Move],
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<(PruningStats, Vec<u8>), String> {
    let solved = spec.solved_index();
    build_pruning_table_from_index(
        spec,
        tables,
        max_entries,
        progress_interval,
        solved,
        moves,
        cancel,
        report,
    )
}

fn build_pruning_table_with_moves_threaded(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    moves: &[Move],
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<(PruningStats, Vec<u8>), String> {
    let solved = spec.solved_index();
    build_pruning_table_from_index_threaded(
        spec,
        tables,
        max_entries,
        progress_interval,
        solved,
        moves,
        threads,
        cancel,
        report,
    )
}

fn build_pruning_table_from_index_threaded(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    root_index: usize,
    moves: &[Move],
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<(PruningStats, Vec<u8>), String> {
    if threads <= 1 {
        return build_pruning_table_from_index(
            spec,
            tables,
            max_entries,
            progress_interval,
            root_index,
            moves,
            cancel,
            report,
        );
    }
    build_pruning_table_from_index_parallel(
        spec,
        tables,
        max_entries,
        progress_interval,
        root_index,
        moves,
        threads,
        cancel,
        report,
    )
}

#[cfg(feature = "gpu")]
fn try_gpu_layer(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    moves: &[Move],
    table: &[AtomicU8],
    depth: u8,
    threads: usize,
    engine: &mut Option<crate::acceleration::PruningCompute>,
    cancel: Option<&AtomicBool>,
    start: usize,
    end: usize,
    reached: usize,
    expanded: usize,
    report: Option<&PruningReporter<'_>>,
) -> Result<Option<(Vec<u32>, bool)>, String> {
    use crate::acceleration;
    if !acceleration::enabled() {
        if engine.take().is_some() {
            acceleration::log(
                "generation backend: CPU; GPU disabled by user; completed batches retained",
            );
        }
        return Ok(None);
    }
    if table.len() > u32::MAX as usize {
        return Ok(None);
    }
    let frontier: Vec<_> = (start..end)
        .filter(|&i| table[i].load(Ordering::Relaxed) == depth)
        .map(|i| i as u32)
        .collect();
    if frontier.is_empty() || (!acceleration::forced() && engine.is_none() && frontier.len() < 256)
    {
        return Ok(None);
    }
    let mut committed = Vec::new();
    let attempt = (|| -> Result<bool, String> {
        if engine.is_none() {
            let Some(gpu) = acceleration::gpu() else {
                return Err("GPU unavailable".to_owned());
            };
            let dimensions: Vec<_> = spec.components.iter().map(|c| c.size() as u32).collect();
            let count: usize = spec.components.iter().map(|c| c.size() * moves.len()).sum();
            if count as u64 * 4 > gpu.binding_limit {
                return Err("GPU transition data exceeds device memory limits".into());
            }
            let mut transitions = Vec::new();
            transitions
                .try_reserve_exact(count)
                .map_err(|_| "not enough RAM for GPU transitions")?;
            for component in &spec.components {
                for index in 0..component.size() {
                    if index & 1023 == 0 && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                        return Err(CANCELLED.into());
                    }
                    for &mv in moves {
                        transitions.push(component.next(index as u32, mv, tables));
                    }
                }
            }
            acceleration::check_host_allocation(table.len().div_ceil(32) as u64 * 4)?;
            let mut visited = vec![0_u32; table.len().div_ceil(32)];
            for (i, v) in table.iter().enumerate() {
                if v.load(Ordering::Relaxed) != UNVISITED {
                    visited[i / 32] |= 1 << (i % 32);
                }
            }
            *engine = acceleration::PruningCompute::new(
                gpu,
                &dimensions,
                &transitions,
                &visited,
                moves.len(),
            )?;
            if engine.is_none() {
                return Err("GPU working set exceeds the safe memory budget".into());
            }
        }

        let mut processed = 0;
        while processed < frontier.len() {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(CANCELLED.into());
            }
            let capacity = engine.as_ref().unwrap().capacity();
            let sample = &frontier[processed..(processed + capacity).min(frontier.len())];
            let cpu_new;
            let cpu_time;
            {
                let count = sample.len();
                let (elapsed, changed) = thread::scope(|scope| {
                    let chunk_size = count.div_ceil(threads.max(1));
                    let handles: Vec<_> = sample
                        .chunks(chunk_size)
                        .map(|chunk| {
                            scope.spawn(move || {
                                let mut changed = Vec::new();
                                let mut values = vec![0; spec.components.len()];
                                let mut next_values = values.clone();
                                let started = Instant::now();
                                for &index in chunk {
                                    for &mv in moves {
                                        let child = spec.next_index(
                                            index as usize,
                                            mv,
                                            tables,
                                            &mut values,
                                            &mut next_values,
                                        );
                                        if table[child]
                                            .compare_exchange(
                                                UNVISITED,
                                                depth + 1,
                                                Ordering::Relaxed,
                                                Ordering::Relaxed,
                                            )
                                            .is_ok()
                                        {
                                            changed.push(child);
                                        }
                                    }
                                }
                                (started.elapsed(), changed)
                            })
                        })
                        .collect();
                    let mut elapsed = std::time::Duration::ZERO;
                    let mut changed = Vec::new();
                    for handle in handles {
                        let (time, items) = handle.join().expect("CPU timing worker panicked");
                        elapsed = elapsed.max(time);
                        changed.extend(items);
                    }
                    (elapsed, changed)
                });
                cpu_new = changed.len();
                cpu_time = elapsed;
                for index in changed {
                    table[index].store(UNVISITED, Ordering::Relaxed);
                }
            }
            let (discoveries, gpu_time) = engine.as_mut().unwrap().expand(sample, cancel)?;
            for &index in &discoveries {
                if index as usize >= table.len()
                    || table[index as usize].load(Ordering::Relaxed) != UNVISITED
                    || depth == 254
                {
                    return Err("GPU returned an invalid discovery".into());
                }
            }
            for &index in &discoveries {
                table[index as usize].store(depth + 1, Ordering::Relaxed);
            }
            let gpu_new = discoveries.len();
            committed.extend(discoveries);
            processed += sample.len();
            if let Some(report) = report {
                report(PruningProgress {
                    name: spec.name(),
                    depth: depth.into(),
                    expanded: expanded + processed,
                    reached: reached + committed.len(),
                    total: table.len(),
                });
            }
            if !acceleration::forced() {
                let cpu_rate = cpu_new.max(1) as f64 / cpu_time.as_secs_f64().max(1e-9);
                let gpu_rate = gpu_new.max(1) as f64 / gpu_time.as_secs_f64().max(1e-9);
                acceleration::log(&format!(
                    "GPU timing: CPU {cpu_rate:.0} vs GPU {gpu_rate:.0} new nodes/s ({} input nodes)",
                    sample.len()
                ));
                if gpu_rate < cpu_rate {
                    acceleration::fallback(&format!(
                        "GPU is slower than CPU ({gpu_rate:.0} vs {cpu_rate:.0} new nodes/s)"
                    ));
                    *engine = None;
                    return Ok(processed == frontier.len());
                }
            }
        }
        Ok(true)
    })();
    match attempt {
        Ok(complete) => Ok(Some((committed, complete))),
        Err(error) if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) => Err(error),
        Err(error) => {
            *engine = None;
            acceleration::fallback(&error);
            Ok(Some((committed, false)))
        }
    }
}

fn full_coordinate_reachability(spec: &CandidateSpec, moves: &[Move]) -> bool {
    if !(0..8).all(|axis| moves.iter().any(|mv| usize::from(mv.axis()) == axis)) {
        return false;
    }
    let mut orbits = [false; 4];
    for component in &spec.components {
        let orbit = match component {
            Component::Corner => 0,
            Component::E0
            | Component::E1
            | Component::E2
            | Component::E3
            | Component::Edge3
            | Component::Edge4 => 1,
            Component::UfCenter
            | Component::UfCenter2
            | Component::UfCenter3
            | Component::LlUfCenterA
            | Component::LlUfCenterB => 2,
            Component::RlCenter | Component::RlCenter2 | Component::RlCenter3 => 3,
        };
        if orbits[orbit] {
            return false;
        }
        orbits[orbit] = true;
    }
    true
}

// Search backward to an exact entry, increasing the TOTAL solution bound.
// Missing entries have lower bound completed_depth + 1, not heuristic 255.
// Exhausting the budget abandons this case, never treating it as unreachable.
fn bounded_reverse_path(
    start: usize,
    completed_depth: u8,
    moves: &[Move],
    get: &impl Fn(usize) -> u8,
    next: &mut impl FnMut(usize, Move) -> usize,
    cancel: Option<&AtomicBool>,
) -> Result<Option<(u8, Vec<usize>)>, String> {
    if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
        return Err(CANCELLED.to_owned());
    }
    fn visit(
        path: &mut Vec<usize>,
        bound: u16,
        lower: u16,
        moves: &[Move],
        get: &impl Fn(usize) -> u8,
        next: &mut impl FnMut(usize, Move) -> usize,
        budget: &mut usize,
        cancel: Option<&AtomicBool>,
    ) -> Result<bool, String> {
        let idx = *path.last().unwrap();
        let distance = get(idx);
        let spent = (path.len() - 1) as u16;
        if distance != UNVISITED {
            return Ok(spent + u16::from(distance) <= bound);
        }
        if spent + lower > bound {
            return Ok(false);
        }
        for &mv in moves {
            if *budget == 0 {
                return Ok(false);
            }
            *budget -= 1;
            if *budget & 63 == 0 && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(CANCELLED.to_owned());
            }
            let previous = next(idx, mv.inverse());
            if path.contains(&previous) {
                continue;
            }
            path.push(previous);
            if visit(path, bound, lower, moves, get, next, budget, cancel)? {
                return Ok(true);
            }
            path.pop();
            if *budget == 0 {
                return Ok(false);
            }
        }
        Ok(false)
    }
    let lower = u16::from(completed_depth) + 1;
    let mut budget = 2048;
    for bound in lower..=(lower + 3).min(254) {
        let mut path = vec![start];
        if visit(
            &mut path,
            bound,
            lower,
            moves,
            get,
            next,
            &mut budget,
            cancel,
        )? {
            // Every intermediate node lies on a proven shortest path too.
            let distance = (path.len() - 1) as u8 + get(*path.last().unwrap());
            return Ok(Some((distance, path)));
        }
        if budget == 0 {
            break;
        }
    }
    Ok(None)
}

// Read-only trial against the actual recent BFS discovery rate. Failed trials
// are bounded, and only a winning trial is committed by the caller.
fn probe_reverse_paths(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    moves: &[Move],
    size: usize,
    seed: usize,
    depth: u8,
    bfs_rate: f64,
    get: impl Fn(usize) -> u8,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<(usize, u8)>, String> {
    let started = Instant::now();
    let mut found = Vec::new();
    let mut tried = 0;
    let mut values = vec![0; spec.components.len()];
    let mut next_values = values.clone();
    let mut next = |idx, mv| spec.next_index(idx, mv, tables, &mut values, &mut next_values);
    for sample in 0..size.min(1024) {
        let idx = (sample * (size / size.min(1024)) + seed) % size;
        if get(idx) != UNVISITED {
            continue;
        }
        tried += 1;
        if let Some((distance, path)) =
            bounded_reverse_path(idx, depth, moves, &get, &mut next, cancel)?
        {
            for (step, node) in path.into_iter().enumerate() {
                if get(node) == UNVISITED && !found.iter().any(|&(other, _)| other == node) {
                    found.push((node, distance - step as u8));
                }
            }
        }
        if tried == 4 {
            break;
        }
    }
    let rate = found.len() as f64 / started.elapsed().as_secs_f64().max(1e-9);
    if rate > bfs_rate * 1.25 {
        Ok(found)
    } else {
        Ok(Vec::new())
    }
}

fn build_pruning_table_from_index(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    root_index: usize,
    moves: &[Move],
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<(PruningStats, Vec<u8>), String> {
    build_pruning_table_from_index_parallel(
        spec,
        tables,
        max_entries,
        progress_interval,
        root_index,
        moves,
        1,
        cancel,
        report,
    )
}

fn build_pruning_table_from_index_parallel(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    root_index: usize,
    moves: &[Move],
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<(PruningStats, Vec<u8>), String> {
    build_pruning_table_from_goals(
        spec,
        tables,
        max_entries,
        progress_interval,
        &[vec![u32::try_from(root_index).map_err(
            |_| "pruning root index exceeds the supported range",
        )?]],
        moves,
        threads,
        cancel,
        report,
        None,
        false,
    )
}

/// Joint-coordinate, multi-source reverse BFS for a partially specified goal.
/// `goals` contains one list per component, or a single packed-index list.
pub(crate) fn build_pruning_table_from_goals(
    spec: &CandidateSpec,
    tables: &TransitionTables,
    max_entries: usize,
    progress_interval: usize,
    goals: &[Vec<u32>],
    moves: &[Move],
    threads: usize,
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
    depth_limit: Option<u8>,
    reverse: bool,
) -> Result<(PruningStats, Vec<u8>), String> {
    let reversed: Vec<_> = moves.iter().map(|m| m.inverse()).collect();
    let moves = if reverse { reversed.as_slice() } else { moves };
    let _stage = crate::acceleration::stage(&spec.name());
    let size = spec
        .size()
        .ok_or_else(|| format!("{} size overflows usize", spec.name()))?;
    if size > max_entries {
        return Err(format!(
            "{} has {size} entries, above cap {max_entries}",
            spec.name()
        ));
    }

    crate::acceleration::check_host_allocation(size as u64)?;
    let table = (0..size)
        .map(|_| AtomicU8::new(UNVISITED))
        .collect::<Vec<_>>();
    let mut root_count = 0;
    if goals.len() == 1 {
        for &index in &goals[0] {
            table[index as usize].store(0, Ordering::Relaxed);
            root_count += 1;
        }
    } else {
        if goals.len() != spec.components.len() || goals.iter().any(Vec::is_empty) {
            return Err("invalid partial goal seeds".into());
        }
        let mut cursors = vec![0; goals.len()];
        loop {
            if root_count & 8191 == 0 && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(CANCELLED.into());
            }
            let mut index = 0;
            for (i, component) in spec.components.iter().enumerate() {
                index = index * component.size() + goals[i][cursors[i]] as usize;
            }
            table[index].store(0, Ordering::Relaxed);
            root_count += 1;
            let mut i = goals.len();
            while i > 0 {
                i -= 1;
                cursors[i] += 1;
                if cursors[i] < goals[i].len() {
                    break;
                }
                cursors[i] = 0;
            }
            if i == 0 && cursors[0] == 0 {
                break;
            }
        }
    }
    if root_count == 0 {
        return Err("the partial goal has no seeds".into());
    }

    let worker_count = threads.min(size).max(1);
    // Bounded CPU windows let a GPU request interrupt the current BFS depth.
    let chunk_size = 65_536;
    let window_size = chunk_size * worker_count;
    let mut reached = root_count;
    let mut histogram = vec![root_count];
    let mut depth_sum = 0_u64;
    let mut max_depth = 0_u8;
    let mut expanded = 0_usize;
    let mut next_progress = progress_interval;
    let tail_allowed = !reverse && full_coordinate_reachability(spec, moves);

    if let Some(report) = report {
        report(PruningProgress {
            name: spec.name(),
            depth: 0,
            expanded: 0,
            reached: root_count,
            total: size,
        });
    }
    #[cfg(feature = "gpu")]
    let mut gpu_layer = None;
    let mut last_window_report = Instant::now();
    for depth in 0..depth_limit.unwrap_or(u8::MAX) {
        if reached == size {
            break;
        }
        if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
            return Err(CANCELLED.to_owned());
        }

        let remaining = AtomicUsize::new(size - reached);
        let can_fill = histogram[usize::from(depth)].saturating_mul(moves.len()) >= size - reached;
        let track_remaining = can_fill || (tail_allowed && reached >= size / 2);
        let next_depth = depth + 1;
        let mut depth_expanded = 0_usize;
        let mut next_frontier = 0_usize;
        let mut depth_sum_delta = 0_u64;

        for window_start in (0..size).step_by(window_size) {
            if track_remaining && remaining.load(Ordering::Relaxed) == 0 {
                break;
            }
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(CANCELLED.to_owned());
            }
            let window_end = (window_start + window_size).min(size);
            // Sparse early layers should not spawn workers for empty windows.
            if !table[window_start..window_end]
                .iter()
                .any(|v| v.load(Ordering::Relaxed) == depth)
            {
                continue;
            }
            #[cfg(feature = "gpu")]
            let gpu_result = try_gpu_layer(
                spec,
                tables,
                moves,
                &table,
                depth,
                threads,
                &mut gpu_layer,
                cancel,
                window_start,
                window_end,
                reached + next_frontier,
                expanded + depth_expanded,
                report,
            )?;
            #[cfg(not(feature = "gpu"))]
            let gpu_result: Option<(Vec<u32>, bool)> = None;
            if let Some((discoveries, complete)) = gpu_result {
                let added = discoveries.len();
                next_frontier += added;
                depth_sum_delta += added as u64 * u64::from(next_depth);
                if track_remaining {
                    remaining.fetch_sub(added, Ordering::Relaxed);
                }
                histogram.resize(histogram.len().max(usize::from(next_depth) + 1), 0);
                histogram[usize::from(next_depth)] += added;
                if added > 0 {
                    max_depth = max_depth.max(next_depth);
                }
                if complete {
                    depth_expanded += (window_start..window_end)
                        .filter(|&i| table[i].load(Ordering::Relaxed) == depth)
                        .count();
                    continue;
                }
            }
            let progress_reached = AtomicUsize::new(reached + next_frontier);
            let progress_expanded = AtomicUsize::new(expanded + depth_expanded);
            let progress_clock = std::sync::Mutex::new(Instant::now());
            thread::scope(|scope| -> Result<(), String> {
                let mut handles = Vec::with_capacity(worker_count);
                for worker in 0..worker_count {
                    let start = window_start + worker * chunk_size;
                    let end = (start + chunk_size).min(window_end);
                    if start >= end {
                        continue;
                    }
                    let table = &table;
                    let remaining = &remaining;
                    let progress_reached = &progress_reached;
                    let progress_expanded = &progress_expanded;
                    let progress_clock = &progress_clock;
                    handles.push(scope.spawn(move || {
                        let mut values = vec![0; spec.components.len()];
                        let mut next_values = vec![0; spec.components.len()];
                        let mut local_expanded = 0_usize;
                        let mut local_reached = 0_usize;
                        let mut local_depth_sum = 0_u64;
                        let mut local_histogram = vec![0_usize; 255];
                        let mut window_start = Instant::now();
                        let mut window_reached = 0_usize;
                        let mut reported_reached = 0;
                        let mut reported_expanded = 0;
                        for idx in start..end {
                            if idx & 8191 == 0 {
                                if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
                                    return Err(CANCELLED.to_owned());
                                }
                                if track_remaining && remaining.load(Ordering::Relaxed) == 0 {
                                    break;
                                }
                            }
                            if table[idx].load(Ordering::Relaxed) != depth {
                                continue;
                            }
                            if track_remaining && remaining.load(Ordering::Relaxed) == 0 {
                                break;
                            }
                            local_expanded += 1;
                            if (local_expanded & 8191) == 0
                                && cancel.is_some_and(|token| token.load(Ordering::Relaxed))
                            {
                                return Err(CANCELLED.to_owned());
                            }
                            for &mv in moves {
                                let next =
                                    spec.next_index(idx, mv, tables, &mut values, &mut next_values);
                                if table[next].load(Ordering::Relaxed) == UNVISITED
                                    && table[next]
                                        .compare_exchange(
                                            UNVISITED,
                                            next_depth,
                                            Ordering::Relaxed,
                                            Ordering::Relaxed,
                                        )
                                        .is_ok()
                                {
                                    if next_depth == UNVISITED {
                                        return Err(
                                        "pruning distance exceeds the table's byte representation"
                                            .to_owned(),
                                    );
                                    }
                                    local_reached += 1;
                                    local_depth_sum += u64::from(next_depth);
                                    local_histogram[usize::from(next_depth)] += 1;
                                    let last = track_remaining
                                        && remaining.fetch_sub(1, Ordering::Relaxed) == 1;
                                    if last {
                                        break;
                                    }
                                }
                            }
                            if local_expanded % 2048 == 0 {
                                let new = local_reached - window_reached;
                                let bfs_rate =
                                    new as f64 / window_start.elapsed().as_secs_f64().max(1e-9);
                                if tail_allowed
                                    && track_remaining
                                    && new < 512
                                    && remaining.load(Ordering::Relaxed) < size / 2
                                {
                                    // Try small batches inside this unfinished BFS layer.
                                    // Recheck throughput after each batch and resume BFS if it loses.
                                    for batch in 0..16 {
                                        let started = Instant::now();
                                        let paths = probe_reverse_paths(
                                            spec,
                                            tables,
                                            moves,
                                            size,
                                            idx.wrapping_add(batch * 7919),
                                            depth,
                                            bfs_rate,
                                            |node| table[node].load(Ordering::Relaxed),
                                            cancel,
                                        )?;
                                        let mut added = 0;
                                        for (node, distance) in paths {
                                            if table[node]
                                                .compare_exchange(
                                                    UNVISITED,
                                                    distance,
                                                    Ordering::Relaxed,
                                                    Ordering::Relaxed,
                                                )
                                                .is_ok()
                                            {
                                                added += 1;
                                                local_reached += 1;
                                                local_depth_sum += u64::from(distance);
                                                local_histogram[usize::from(distance)] += 1;
                                                remaining.fetch_sub(1, Ordering::Relaxed);
                                            }
                                        }
                                        if added == 0
                                            || added as f64
                                                / started.elapsed().as_secs_f64().max(1e-9)
                                                <= bfs_rate * 1.25
                                        {
                                            break;
                                        }
                                    }
                                }
                                window_start = Instant::now();
                                window_reached = local_reached;
                            }
                            if local_expanded % 256 == 0 {
                                progress_reached
                                    .fetch_add(local_reached - reported_reached, Ordering::Relaxed);
                                progress_expanded.fetch_add(
                                    local_expanded - reported_expanded,
                                    Ordering::Relaxed,
                                );
                                reported_reached = local_reached;
                                reported_expanded = local_expanded;
                                if let Ok(mut clock) = progress_clock.try_lock() {
                                    if clock.elapsed().as_millis() >= 200 {
                                        if let Some(report) = report {
                                            report(PruningProgress {
                                                name: spec.name(),
                                                depth: depth.into(),
                                                expanded: progress_expanded.load(Ordering::Relaxed),
                                                reached: progress_reached.load(Ordering::Relaxed),
                                                total: size,
                                            });
                                        }
                                        *clock = Instant::now();
                                    }
                                }
                            }
                        }
                        Ok((
                            local_expanded,
                            local_reached,
                            local_depth_sum,
                            local_histogram,
                        ))
                    }));
                }

                for handle in handles {
                    let (local_expanded, local_reached, local_depth_sum, local_histogram) = handle
                        .join()
                        .map_err(|_| "parallel pruning worker panicked".to_owned())??;
                    depth_expanded += local_expanded;
                    next_frontier += local_reached;
                    depth_sum_delta += local_depth_sum;
                    for (distance, count) in local_histogram.into_iter().enumerate() {
                        if count > 0 {
                            histogram.resize(histogram.len().max(distance + 1), 0);
                            histogram[distance] += count;
                            max_depth = max_depth.max(distance as u8);
                        }
                    }
                }
                Ok(())
            })?;
            if last_window_report.elapsed().as_millis() >= 200 {
                if let Some(report) = report {
                    report(PruningProgress {
                        name: spec.name(),
                        depth: depth.into(),
                        expanded: expanded + depth_expanded,
                        reached: reached + next_frontier,
                        total: size,
                    });
                }
                last_window_report = Instant::now();
            }
        }
        expanded += depth_expanded;
        reached += next_frontier;
        depth_sum += depth_sum_delta;
        if progress_interval > 0 && expanded >= next_progress {
            eprintln!(
                "  {}: expanded {}, reached {}, depth {}",
                spec.name(),
                expanded,
                reached,
                depth
            );
            if let Some(report) = report {
                report(PruningProgress {
                    name: spec.name(),
                    depth: depth.into(),
                    expanded,
                    reached,
                    total: size,
                });
            }
            next_progress = expanded
                .saturating_add(progress_interval)
                .max(next_progress.saturating_add(progress_interval));
        }

        if progress_interval > 0 {
            eprintln!(
                "  {}: finished depth {}, new {}, reached {}",
                spec.name(),
                depth,
                next_frontier,
                reached
            );
        }
        if histogram.get(usize::from(next_depth)).copied().unwrap_or(0) == 0 {
            break;
        }
    }

    let table = table
        .into_iter()
        .map(|value| value.into_inner())
        .collect::<Vec<_>>();
    let stats = PruningStats {
        name: spec.name(),
        table_entries: size,
        table_bytes: size,
        reached,
        max_depth,
        average_depth_milli: ((depth_sum * 1000) / reached as u64) as u32,
        histogram,
    };
    Ok((stats, table))
}

fn write_table(path: impl AsRef<Path>, table: &[u8]) -> io::Result<()> {
    write_table_with_metadata(path, table, "")
}

fn write_table_with_metadata(
    path: impl AsRef<Path>,
    table: &[u8],
    metadata: &str,
) -> io::Result<()> {
    write_table_with_metadata_reporting(path, table, metadata, None, "", None)
}

pub(crate) fn write_table_with_metadata_reporting(
    path: impl AsRef<Path>,
    table: &[u8],
    metadata: &str,
    report: Option<&PruningReporter<'_>>,
    progress_name: &str,
    cancel: Option<&AtomicBool>,
) -> io::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pending = crate::cache_file::CacheWrite::new(path.as_ref())?;
    let mut writer = BufWriter::new(File::create(pending.path())?);
    let total = table.len().max(1);
    report_table_io(report, progress_name, 0, total);
    writer.write_all(PRUN_MAGIC)?;
    writer.write_all(&(metadata.len() as u32).to_le_bytes())?;
    writer.write_all(metadata.as_bytes())?;
    let chunk_size = (total / 100).max(1024 * 1024).min(total);
    let mut written = 0_usize;
    for chunk in table.chunks(chunk_size) {
        check_table_cancel(cancel)?;
        writer.write_all(chunk)?;
        written += chunk.len();
        report_table_io(report, progress_name, written.min(total), total);
    }
    writer.flush()?;
    check_table_cancel(cancel)?;
    drop(writer);
    pending.commit()
}

fn read_table(path: impl AsRef<Path>, expected_len: usize) -> io::Result<Vec<u8>> {
    read_table_reporting(path, expected_len, None, "", None)
}

pub(crate) fn read_table_reporting(
    path: impl AsRef<Path>,
    expected_len: usize,
    report: Option<&PruningReporter<'_>>,
    progress_name: &str,
    cancel: Option<&AtomicBool>,
) -> io::Result<Vec<u8>> {
    check_table_cancel(cancel)?;
    let mut file = File::open(path)?;
    let total = file
        .metadata()?
        .len()
        .try_into()
        .unwrap_or(usize::MAX)
        .max(1);
    report_table_io(report, progress_name, 0, total);
    let mut table = Vec::with_capacity(total.min(expected_len.saturating_add(4096)));
    let chunk_size = (total / 100).max(1024 * 1024).min(total);
    let mut buffer = vec![0; chunk_size];
    let mut read = 0_usize;
    loop {
        check_table_cancel(cancel)?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        table.extend_from_slice(&buffer[..n]);
        read = read.saturating_add(n);
        report_table_io(report, progress_name, read.min(total), total);
    }
    if table.starts_with(PRUN_MAGIC) {
        if table.len() < PRUN_MAGIC.len() + 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pruning table header is truncated",
            ));
        }
        let mut len_bytes = [0; 4];
        len_bytes.copy_from_slice(&table[PRUN_MAGIC.len()..PRUN_MAGIC.len() + 4]);
        let metadata_len = u32::from_le_bytes(len_bytes) as usize;
        let table_start = PRUN_MAGIC.len() + 4 + metadata_len;
        if table_start > table.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pruning table metadata is truncated",
            ));
        }
        table = table[table_start..].to_vec();
    }
    if table.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pruning table length mismatch",
        ));
    }
    Ok(table)
}

pub fn read_table_metadata(path: impl AsRef<Path>) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut magic = [0; 16];
    file.read_exact(&mut magic)?;
    if &magic != PRUN_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pruning table metadata magic mismatch",
        ));
    }
    let mut len_bytes = [0; 4];
    file.read_exact(&mut len_bytes)?;
    let metadata_len = u32::from_le_bytes(len_bytes) as usize;
    let mut metadata = vec![0; metadata_len];
    file.read_exact(&mut metadata)?;
    String::from_utf8(metadata).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[must_use]
pub fn default_candidates() -> Vec<CandidateSpec> {
    use Component::{
        Corner, E0, E1, E2, E3, Edge3, Edge4, RlCenter, RlCenter2, RlCenter3, UfCenter, UfCenter2,
        UfCenter3,
    };

    vec![
        CandidateSpec::new(vec![Corner]),
        CandidateSpec::new(vec![E0]),
        CandidateSpec::new(vec![E1]),
        CandidateSpec::new(vec![E2]),
        CandidateSpec::new(vec![E3]),
        CandidateSpec::new(vec![Edge3]),
        CandidateSpec::new(vec![Edge4]),
        CandidateSpec::new(vec![UfCenter]),
        CandidateSpec::new(vec![RlCenter]),
        CandidateSpec::new(vec![UfCenter2]),
        CandidateSpec::new(vec![UfCenter3]),
        CandidateSpec::new(vec![RlCenter2]),
        CandidateSpec::new(vec![RlCenter3]),
        CandidateSpec::new(vec![E0, E1]),
        CandidateSpec::new(vec![E0, E2]),
        CandidateSpec::new(vec![E0, E3]),
        CandidateSpec::new(vec![E1, E2]),
        CandidateSpec::new(vec![E1, E3]),
        CandidateSpec::new(vec![E2, E3]),
        CandidateSpec::new(vec![Corner, E0]),
        CandidateSpec::new(vec![Corner, E1]),
        CandidateSpec::new(vec![Corner, E2]),
        CandidateSpec::new(vec![Corner, E3]),
        CandidateSpec::new(vec![E0, UfCenter]),
        CandidateSpec::new(vec![E1, UfCenter]),
        CandidateSpec::new(vec![E2, UfCenter]),
        CandidateSpec::new(vec![E3, UfCenter]),
        CandidateSpec::new(vec![E0, RlCenter]),
        CandidateSpec::new(vec![E1, RlCenter]),
        CandidateSpec::new(vec![E2, RlCenter]),
        CandidateSpec::new(vec![E3, RlCenter]),
        CandidateSpec::new(vec![E0, E1, E2]),
        CandidateSpec::new(vec![E0, E1, E3]),
        CandidateSpec::new(vec![E0, E2, E3]),
        CandidateSpec::new(vec![E1, E2, E3]),
        CandidateSpec::new(vec![Edge3, UfCenter2]),
        CandidateSpec::new(vec![Edge3, UfCenter3]),
        CandidateSpec::new(vec![Edge3, RlCenter2]),
        CandidateSpec::new(vec![Edge3, RlCenter3]),
        CandidateSpec::new(vec![Edge4, UfCenter2]),
        CandidateSpec::new(vec![Edge4, RlCenter2]),
    ]
}

#[must_use]
pub fn all_components() -> [Component; 13] {
    use Component::{
        Corner, E0, E1, E2, E3, Edge3, Edge4, RlCenter, RlCenter2, RlCenter3, UfCenter, UfCenter2,
        UfCenter3,
    };
    [
        Corner, E0, E1, E2, E3, Edge3, Edge4, UfCenter, RlCenter, UfCenter2, UfCenter3, RlCenter2,
        RlCenter3,
    ]
}

#[must_use]
pub fn generated_candidates(max_components: usize, max_entries: usize) -> Vec<CandidateSpec> {
    let components = all_components();
    let mut out = Vec::new();
    for width in 1..=max_components {
        push_combinations(
            &components,
            width,
            0,
            &mut Vec::new(),
            max_entries,
            &mut out,
        );
    }
    out
}

fn push_combinations(
    components: &[Component],
    width: usize,
    start: usize,
    current: &mut Vec<Component>,
    max_entries: usize,
    out: &mut Vec<CandidateSpec>,
) {
    if current.len() == width {
        let spec = CandidateSpec::new(current.clone());
        if spec.size().is_some_and(|size| size <= max_entries) {
            out.push(spec);
        }
        return;
    }

    for i in start..components.len() {
        current.push(components[i]);
        push_combinations(components, width, i + 1, current, max_entries, out);
        current.pop();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires GPU compute (software adapters may be enabled for parity tests)"]
    #[cfg(feature = "gpu")]
    fn live_cpu_gpu_cpu_handoff_preserves_bfs_progress_and_table() {
        use super::{PruningProgress, build_pruning_table_from_index_threaded};
        use crate::acceleration::{self, GenerationControl, GenerationLog, Policy};
        use std::sync::atomic::Ordering;
        use std::sync::{Arc, Mutex};
        let tables = TransitionTables::corner_only_for_tests();
        let spec = CandidateSpec::new(vec![Component::Corner]);
        let moves = [Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp];
        let baseline = acceleration::with_policy(Policy::Cpu, None, || {
            build_pruning_table_from_index_threaded(
                &spec,
                &tables,
                usize::MAX,
                1,
                spec.solved_index(),
                &moves,
                2,
                None,
                None,
            )
            .unwrap()
        });
        let control = Arc::new(GenerationControl::new(false));
        let gpu_seen = Arc::new(AtomicBool::new(false));
        let fallback_seen = Arc::new(AtomicBool::new(false));
        let seen = gpu_seen.clone();
        let log: Arc<GenerationLog> = Arc::new(move |s| {
            if s.starts_with("generation backend: GPU") {
                seen.store(true, Ordering::Relaxed);
            }
        });
        let reached = Mutex::new(Vec::new());
        let report = |p: PruningProgress| {
            reached.lock().unwrap().push(p.reached);
            if gpu_seen.load(Ordering::Relaxed) && !fallback_seen.swap(true, Ordering::Relaxed) {
                acceleration::fallback(
                    "GPU is slower than CPU (injected timing result for handoff parity)",
                );
            } else if !gpu_seen.load(Ordering::Relaxed) {
                control.set_enabled(true);
            }
        };
        let actual = acceleration::with_control(Policy::Gpu, control.clone(), Some(log), || {
            build_pruning_table_from_index_threaded(
                &spec,
                &tables,
                usize::MAX,
                1,
                spec.solved_index(),
                &moves,
                2,
                None,
                Some(&report),
            )
            .unwrap()
        });
        assert!(
            gpu_seen.load(Ordering::Relaxed),
            "GPU compute did not execute"
        );
        assert!(fallback_seen.load(Ordering::Relaxed));
        assert!(!control.enabled());
        assert_eq!(actual.1, baseline.1);
        assert_eq!(actual.0.histogram, baseline.0.histogram);
        assert!(
            reached.lock().unwrap().windows(2).all(|w| w[0] <= w[1]),
            "reached-state progress went backwards during handoff"
        );
    }
    #[test]
    fn bounded_dense_reads_preserve_surviving_values_and_skip_rejected_reads() {
        for first in [0, 1, 7, 254, 255] {
            for second in [0, 2, 8, 254, 255] {
                let p = super::SolverPruning {
                    edge3_uf3: vec![first],
                    corner_uf3: vec![second],
                    compact: None,
                    orbit_swap: None,
                };
                for bound in 0..=255 {
                    let actual = p.heuristic_cached_bounded(0, 0, bound);
                    let exact = first.max(second);
                    assert_eq!(actual > bound, exact > bound);
                    if exact <= bound {
                        assert_eq!(actual, exact);
                    }
                }
            }
        }
        let p = super::SolverPruning {
            edge3_uf3: vec![7],
            corner_uf3: vec![],
            compact: None,
            orbit_swap: None,
        };
        assert_eq!(p.heuristic_cached_bounded(0, usize::MAX, 6), 7);
    }

    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires cached FTO transitions and GPU hardware"]
    fn gpu_pruning_matches_cpu_including_unreachable_states() {
        use crate::acceleration::{self, Policy};
        let tables = crate::tables::TransitionTables::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../cache/transition-table.tran"
        ))
        .unwrap();
        for moves in [
            vec![Move::R, Move::Rp, Move::U, Move::Up],
            Move::ALL.to_vec(),
        ] {
            for spec in [
                CandidateSpec::new(vec![Component::E0, Component::UfCenter2]),
                CandidateSpec::new(vec![Component::E0, Component::E1]),
            ] {
                let started = std::time::Instant::now();
                let cpu = super::build_pruning_table_from_index_threaded(
                    &spec,
                    &tables,
                    usize::MAX,
                    0,
                    spec.solved_index(),
                    &moves,
                    2,
                    None,
                    None,
                )
                .unwrap();
                let cpu_time = started.elapsed();
                let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
                let sink = messages.clone();
                let log: std::sync::Arc<acceleration::GenerationLog> =
                    std::sync::Arc::new(move |s| {
                        println!("{s}");
                        sink.lock().unwrap().push(s.into());
                    });
                let started = std::time::Instant::now();
                let gpu = acceleration::with_policy(Policy::Gpu, Some(log), || {
                    super::build_pruning_table_from_index_threaded(
                        &spec,
                        &tables,
                        usize::MAX,
                        0,
                        spec.solved_index(),
                        &moves,
                        2,
                        None,
                        None,
                    )
                })
                .unwrap();
                assert!(
                    messages
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|s| s.contains("GPU BFS working set")),
                    "GPU did not run"
                );
                assert!(
                    !messages
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|s| s.contains("failed")),
                    "GPU failed and fell back"
                );
                assert_eq!(cpu.1, gpu.1);
                assert_eq!(cpu.0.histogram, gpu.0.histogram);
                println!(
                    "GPU PRUNING PARITY {} moves={} reached={} CPU={cpu_time:?} GPU={:?}",
                    spec.name(),
                    moves.len(),
                    gpu.0.reached,
                    started.elapsed()
                );
            }
        }
    }

    use std::collections::VecDeque;
    use std::sync::atomic::AtomicBool;

    use crate::{
        moves::Move,
        pruning::{CandidateSpec, Component, evaluate_candidate},
        tables::TransitionTables,
    };

    #[test]
    fn reverse_search_fills_paths_across_an_incomplete_layer() {
        // A directed 20-node cycle: BFS knows all distances through eight,
        // plus one unrelated entry of the unfinished ninth-and-later layers.
        let mut partial = [super::UNVISITED; 20];
        for (idx, distance) in partial.iter_mut().enumerate().take(9) {
            *distance = idx as u8;
        }
        partial[15] = 15;
        for start in [9, 10, 11, 12] {
            let (distance, path) = super::bounded_reverse_path(
                start,
                8,
                &[Move::U],
                &|idx| partial[idx],
                &mut |idx, mv| {
                    assert_eq!(mv, Move::Up);
                    (idx + 19) % 20
                },
                None,
            )
            .unwrap()
            .expect("should connect to the completed eighth layer");
            assert_eq!(distance, start as u8);
            assert_eq!(path, (8..=start).rev().collect::<Vec<_>>());
            for (step, idx) in path.into_iter().enumerate() {
                assert_eq!(distance - step as u8, idx as u8);
            }
        }
        assert!(
            super::bounded_reverse_path(
                13,
                8,
                &[Move::U],
                &|idx| partial[idx],
                &mut |idx, _| (idx + 19) % 20,
                None,
            )
            .unwrap()
            .is_none(),
            "a case beyond the lookahead should fall back to BFS"
        );
    }

    #[test]
    fn reverse_search_proves_shortest_distance_and_bounds_dead_ends() {
        // Move U visits an exact but distant anchor first; move F reaches the
        // eighth layer. Accepting the first anchor would overestimate 9 as 12.
        let values = [super::UNVISITED, 11, 8];
        let result = super::bounded_reverse_path(
            0,
            8,
            &[Move::U, Move::F],
            &|idx| values[idx],
            &mut |_, mv| if mv == Move::Up { 1 } else { 2 },
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result, (9, vec![0, 2]));

        let mut steps = 0;
        assert!(
            super::bounded_reverse_path(
                0,
                8,
                &Move::ALL,
                &|_| super::UNVISITED,
                &mut |idx, _| {
                    steps += 1;
                    idx + 1
                },
                None,
            )
            .unwrap()
            .is_none()
        );
        assert!(steps <= 2048);
        let cancel = AtomicBool::new(true);
        assert_eq!(
            super::bounded_reverse_path(
                0,
                8,
                &[Move::U],
                &|_| super::UNVISITED,
                &mut |idx, _| idx + 1,
                Some(&cancel),
            )
            .unwrap_err(),
            super::CANCELLED
        );
    }

    #[test]
    fn reverse_search_requires_a_full_reachability_certificate() {
        let corner = CandidateSpec::new(vec![Component::Corner]);
        assert!(!super::full_coordinate_reachability(
            &corner,
            &[Move::R, Move::U]
        ));
        assert!(!super::full_coordinate_reachability(&corner, &[]));
        assert!(super::full_coordinate_reachability(&corner, &Move::ALL));
        assert!(super::full_coordinate_reachability(
            &CandidateSpec::new(vec![Component::Edge3, Component::UfCenter3]),
            &Move::ALL,
        ));
        assert!(!super::full_coordinate_reachability(
            &CandidateSpec::new(vec![Component::E0, Component::E1]),
            &Move::ALL,
        ));
    }

    #[test]
    #[ignore = "builds corner transition tables; run in release mode"]
    fn adaptive_build_matches_queue_bfs() {
        let tables = TransitionTables::corner_only_for_tests();
        let spec = CandidateSpec::new(vec![Component::Corner]);
        for moves in [
            Move::ALL.to_vec(),
            vec![Move::R, Move::U],
            vec![Move::U],
            vec![],
        ] {
            for root in [spec.solved_index(), 1234] {
                let mut expected = vec![super::UNVISITED; spec.size().unwrap()];
                expected[root] = 0;
                let mut queue = VecDeque::from([root]);
                let mut values = [0];
                let mut next_values = [0];
                while let Some(idx) = queue.pop_front() {
                    for &mv in &moves {
                        let next = spec.next_index(idx, mv, &tables, &mut values, &mut next_values);
                        if expected[next] == super::UNVISITED {
                            expected[next] = expected[idx] + 1;
                            queue.push_back(next);
                        }
                    }
                }
                let mut expected_histogram = vec![0; 1];
                let mut sum = 0_u64;
                for &depth in &expected {
                    if depth != super::UNVISITED {
                        expected_histogram
                            .resize(expected_histogram.len().max(depth as usize + 1), 0);
                        expected_histogram[depth as usize] += 1;
                        sum += u64::from(depth);
                    }
                }
                let reached = expected_histogram.iter().sum::<usize>();
                for threads in [1, 4] {
                    let (stats, actual) = super::build_pruning_table_from_index_threaded(
                        &spec,
                        &tables,
                        usize::MAX,
                        0,
                        root,
                        &moves,
                        threads,
                        None,
                        None,
                    )
                    .unwrap();
                    assert_eq!(
                        actual, expected,
                        "moves {moves:?}, root {root}, threads {threads}"
                    );
                    assert_eq!(stats.reached, reached);
                    assert_eq!(stats.histogram, expected_histogram);
                    assert_eq!(stats.max_depth as usize + 1, expected_histogram.len());
                    assert_eq!(
                        stats.average_depth_milli,
                        ((sum * 1000) / reached as u64) as u32
                    );
                }
            }
        }

        let cancel = AtomicBool::new(true);
        for threads in [1, 4] {
            let result = super::build_pruning_table_from_index_threaded(
                &spec,
                &tables,
                usize::MAX,
                0,
                spec.solved_index(),
                &Move::ALL,
                threads,
                Some(&cancel),
                None,
            );
            assert_eq!(result.unwrap_err(), super::CANCELLED);
        }
    }

    #[test]
    #[ignore = "builds full transition tables"]
    fn evaluates_corner_candidate() {
        let tables = TransitionTables::build();
        let stats = evaluate_candidate(
            &CandidateSpec::new(vec![Component::Corner]),
            &tables,
            usize::MAX,
        )
        .expect("corner candidate should evaluate");

        assert_eq!(stats.table_entries, 11_520);
        assert!(stats.max_depth > 0);
    }

    #[test]
    fn move_set_suffix_round_trips() {
        use crate::moves::{Move, Move::*};
        for set in [
            vec![R, Rp, U, Up, F, Fp],
            vec![RURp, FUpFp, S, Sp, M],
            vec![Dp, E, Lw, B, BRp],
            Move::ALL.to_vec(),
        ] {
            let suffix = super::move_set_suffix(&set);
            let parsed = super::parse_move_set_suffix(&suffix).expect("suffix should parse back");
            let mut parsed_sorted = parsed;
            parsed_sorted.sort_unstable_by_key(|mv| mv.idx());
            let mut set_sorted = set;
            set_sorted.sort_unstable_by_key(|mv| mv.idx());
            assert_eq!(parsed_sorted, set_sorted, "round trip for {suffix}");
        }
    }

    #[test]
    fn move_set_suffix_is_canonical() {
        use crate::moves::Move::*;
        assert_eq!(
            super::move_set_suffix(&[U, R, Up]),
            super::move_set_suffix(&[R, U, Up])
        );
    }
}

fn check_table_cancel(cancel: Option<&AtomicBool>) -> io::Result<()> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err(io::Error::new(io::ErrorKind::Interrupted, CANCELLED))
    } else {
        Ok(())
    }
}
