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
                    "",
                    report,
                    &format!("saving {}", spec.name()),
                )
                .map_err(|error| error.to_string())?;
                Ok(Self { spec, table })
            }
        }
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

    /// Select the move strategy once; dense searches keep their original path.
    pub(crate) fn for_moves(&self) -> Option<&Self> {
        self.compact.as_ref().map(|_| self)
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
    pub(crate) fn dense_reference_for_tests(&self) -> Self {
        let compact = self.compact.as_ref().expect("needs a compact source");
        Self {
            edge3_uf3: compact.expand_for_tests(subgroup::Kind::Edge, &self.edge3_uf3),
            corner_uf3: compact.expand_for_tests(subgroup::Kind::Corner, &self.corner_uf3),
            compact: None,
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
        "loading pruning table {} ({:.1} MiB)",
        spec.name(),
        spec.size().unwrap_or(0) as f64 / (1024.0 * 1024.0)
    );
    match if path.exists() {
        read_table_reporting(
            path,
            spec.size().unwrap_or(0),
            report,
            &format!("loading {}", spec.name()),
        )
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "pruning table does not exist",
        ))
    } {
        Ok(table) => Ok(table),
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
        return "all".to_owned();
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
    if suffix == "all" {
        return Some(Move::ALL.to_vec());
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

// These face turns generate the full FTO group. Only certify products of
// independent piece orbits; overlapping coordinates can contain impossible pairs.
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
    let size = spec
        .size()
        .ok_or_else(|| format!("{} size overflows usize", spec.name()))?;
    if size > max_entries {
        return Err(format!(
            "{} has {size} entries, above cap {max_entries}",
            spec.name()
        ));
    }

    let table = (0..size)
        .map(|_| AtomicU8::new(UNVISITED))
        .collect::<Vec<_>>();
    table[root_index].store(0, Ordering::Relaxed);

    let worker_count = threads.min(size).max(1);
    let chunk_size = size.div_ceil(worker_count);
    let mut reached = 1_usize;
    let mut histogram = vec![1_usize];
    let mut depth_sum = 0_u64;
    let mut max_depth = 0_u8;
    let mut expanded = 0_usize;
    let mut next_progress = progress_interval;
    let tail_allowed = full_coordinate_reachability(spec, moves);

    for depth in 0..u8::MAX {
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

        thread::scope(|scope| -> Result<(), String> {
            let mut handles = Vec::with_capacity(worker_count);
            for worker in 0..worker_count {
                let start = worker * chunk_size;
                let end = ((worker + 1) * chunk_size).min(size);
                if start >= end {
                    continue;
                }
                let table = &table;
                let remaining = &remaining;
                handles.push(scope.spawn(move || {
                    let mut values = vec![0; spec.components.len()];
                    let mut next_values = vec![0; spec.components.len()];
                    let mut local_expanded = 0_usize;
                    let mut local_reached = 0_usize;
                    let mut local_depth_sum = 0_u64;
                    let mut local_histogram = vec![0_usize; 255];
                    let mut window_start = Instant::now();
                    let mut window_reached = 0_usize;
                    let mut local_next_progress = next_progress.saturating_sub(expanded);
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
                                        || added as f64 / started.elapsed().as_secs_f64().max(1e-9)
                                            <= bfs_rate * 1.25
                                    {
                                        break;
                                    }
                                }
                            }
                            window_start = Instant::now();
                            window_reached = local_reached;
                        }
                        if worker_count == 1
                            && progress_interval > 0
                            && local_expanded >= local_next_progress
                        {
                            if let Some(report) = report {
                                report(PruningProgress {
                                    name: spec.name(),
                                    depth: depth.into(),
                                    expanded: expanded + local_expanded,
                                    reached: reached + local_reached,
                                    total: size,
                                });
                            }
                            local_next_progress =
                                local_next_progress.saturating_add(progress_interval);
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
    write_table_with_metadata_reporting(path, table, metadata, None, "")
}

fn write_table_with_metadata_reporting(
    path: impl AsRef<Path>,
    table: &[u8],
    metadata: &str,
    report: Option<&PruningReporter<'_>>,
    progress_name: &str,
) -> io::Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(path)?);
    let total = table.len().max(1);
    report_table_io(report, progress_name, 0, total);
    writer.write_all(PRUN_MAGIC)?;
    writer.write_all(&(metadata.len() as u32).to_le_bytes())?;
    writer.write_all(metadata.as_bytes())?;
    let chunk_size = (total / 100).max(1024 * 1024).min(total);
    let mut written = 0_usize;
    for chunk in table.chunks(chunk_size) {
        writer.write_all(chunk)?;
        written += chunk.len();
        report_table_io(report, progress_name, written.min(total), total);
    }
    writer.flush()
}

fn read_table(path: impl AsRef<Path>, expected_len: usize) -> io::Result<Vec<u8>> {
    read_table_reporting(path, expected_len, None, "")
}

fn read_table_reporting(
    path: impl AsRef<Path>,
    expected_len: usize,
    report: Option<&PruningReporter<'_>>,
    progress_name: &str,
) -> io::Result<Vec<u8>> {
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
