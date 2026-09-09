use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{
    coord::{CENTER3_COUNT, CENTER_COUNT, CORNER_COUNT, EDGE_CHOICE_COUNT},
    moves::Move,
    tables::TransitionTables,
    FtoCoord,
};

const UNVISITED: u8 = u8::MAX;

pub const CANCELLED: &str = "solve cancelled";

#[derive(Clone, Debug)]
pub struct PruningProgress {
    pub name: String,
    pub depth: usize,
    pub expanded: usize,
    pub reached: usize,
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
                let (_, table) = build_pruning_table(&spec, tables, max_entries, progress_interval)?;
                write_table(path, &table).map_err(|error| error.to_string())?;
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
        if value == UNVISITED {
            0
        } else {
            value
        }
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
        Self::load_or_build_with_moves_reporting(
            tables,
            out_dir,
            progress_interval,
            moves,
            None,
            None,
        )
    }

    pub fn load_or_build_with_moves_reporting(
        tables: &TransitionTables,
        out_dir: &Path,
        progress_interval: usize,
        moves: &[Move],
        cancel: Option<&AtomicBool>,
        report: Option<&PruningReporter<'_>>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(out_dir).map_err(|error| error.to_string())?;
        let suffix = move_set_suffix(moves);
        let edge3_uf3 = load_or_build_solver_table(
            CandidateSpec::new(vec![Component::Edge3, Component::UfCenter3]),
            tables,
            progress_interval,
            out_dir.join(format!("edge3__uf3__{suffix}.pdb")),
            moves,
            cancel,
            report,
        )?;
        let corner_uf3 = load_or_build_solver_table(
            CandidateSpec::new(vec![Component::Corner, Component::UfCenter3]),
            tables,
            progress_interval,
            out_dir.join(format!("corner__uf3__{suffix}.pdb")),
            moves,
            cancel,
            report,
        )?;
        Ok(Self {
            edge3_uf3,
            corner_uf3,
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
        let edge3_uf3 = build_solver_table_from_root(
            CandidateSpec::new(vec![Component::Edge3, Component::UfCenter3]),
            root,
            tables,
            progress_interval,
            moves,
        )?;
        let corner_uf3 = build_solver_table_from_root(
            CandidateSpec::new(vec![Component::Corner, Component::UfCenter3]),
            root,
            tables,
            progress_interval,
            moves,
        )?;
        Ok(Self {
            edge3_uf3,
            corner_uf3,
        })
    }

    #[must_use]
    pub fn heuristic(&self, edge3_uf3_idx: usize, corner_uf3_idx: usize) -> u8 {
        live_depth(self.edge3_uf3[edge3_uf3_idx]).max(live_depth(self.corner_uf3[corner_uf3_idx]))
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
        self.edge3_uf3.len() + self.corner_uf3.len()
    }
}

const fn live_depth(value: u8) -> u8 {
    if value == UNVISITED {
        0
    } else {
        value
    }
}

fn load_or_build_solver_table(
    spec: CandidateSpec,
    tables: &TransitionTables,
    progress_interval: usize,
    path: impl AsRef<Path>,
    moves: &[Move],
    cancel: Option<&AtomicBool>,
    report: Option<&PruningReporter<'_>>,
) -> Result<Vec<u8>, String> {
    let path = path.as_ref();
    eprintln!(
        "loading pruning table {} ({:.1} MiB)",
        spec.name(),
        spec.size().unwrap_or(0) as f64 / (1024.0 * 1024.0)
    );
    match read_table(path, spec.size().unwrap_or(0)) {
        Ok(table) => Ok(table),
        Err(_) => {
            let (_, table) = build_pruning_table_with_moves(
                &spec,
                tables,
                usize::MAX,
                progress_interval,
                moves,
                cancel,
                report,
            )?;
            write_table(path, &table).map_err(|error| error.to_string())?;
            Ok(table)
        }
    }
}

fn build_solver_table_from_root(
    spec: CandidateSpec,
    root: FtoCoord,
    tables: &TransitionTables,
    progress_interval: usize,
    moves: &[Move],
) -> Result<Vec<u8>, String> {
    let root_index = spec.index_of_coord(root);
    let (_, table) = build_pruning_table_from_index(
        &spec,
        tables,
        usize::MAX,
        progress_interval,
        root_index,
        moves,
        None,
        None,
    )?;
    Ok(table)
}

fn move_set_suffix(moves: &[Move]) -> String {
    if moves == Move::ALL.as_slice() {
        return "all".to_owned();
    }
    let mut out = String::new();
    for &mv in moves {
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(
            &mv.name()
                .replace('\'', "p")
                .replace(' ', "")
                .replace("BR", "br")
                .replace("BL", "bl"),
        );
    }
    out
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
            Self::UfCenter3 | Self::RlCenter3 => crate::coord::CENTER3_COUNT,
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
    build_pruning_table_with_moves(spec, tables, max_entries, progress_interval, &Move::ALL, None, None)
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
    build_pruning_table_from_index(spec, tables, max_entries, progress_interval, solved, moves, cancel, report)
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
    let size = spec
        .size()
        .ok_or_else(|| format!("{} size overflows usize", spec.name()))?;
    if size > max_entries {
        return Err(format!(
            "{} has {size} entries, above cap {max_entries}",
            spec.name()
        ));
    }

    let mut table = vec![UNVISITED; size];
    table[root_index] = 0;

    let mut values = vec![0; spec.components.len()];
    let mut next_values = vec![0; spec.components.len()];
    let mut reached = 1_usize;
    let mut histogram = vec![1_usize];
    let mut depth_sum = 0_u64;
    let mut max_depth = 0_u8;
    let mut expanded = 0_usize;
    let mut next_progress = progress_interval;
    let cancel_check_mask: usize = 8191;

    for depth in 0..u8::MAX {
        if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
            return Err(CANCELLED.to_owned());
        }
        let mut next_frontier = 0_usize;
        for idx in 0..size {
            if table[idx] != depth {
                continue;
            }
            expanded += 1;
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
                    });
                }
                next_progress = next_progress.saturating_add(progress_interval);
                if cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
                    return Err(CANCELLED.to_owned());
                }
            }
            if expanded & cancel_check_mask == 0 && cancel.is_some_and(|token| token.load(Ordering::Relaxed)) {
                return Err(CANCELLED.to_owned());
            }
            for &mv in moves {
                let next = spec.next_index(idx, mv, tables, &mut values, &mut next_values);
                if table[next] == UNVISITED {
                    let next_depth = depth + 1;
                    table[next] = next_depth;
                    reached += 1;
                    next_frontier += 1;
                    depth_sum += u64::from(next_depth);
                    max_depth = max_depth.max(next_depth);
                    let hist_idx = usize::from(next_depth);
                    if histogram.len() <= hist_idx {
                        histogram.resize(hist_idx + 1, 0);
                    }
                    histogram[hist_idx] += 1;
                }
            }
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
        if next_frontier == 0 {
            break;
        }
    }

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
    if let Some(parent) = path.as_ref().parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(table)?;
    writer.flush()
}

fn read_table(path: impl AsRef<Path>, expected_len: usize) -> io::Result<Vec<u8>> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut table = Vec::new();
    reader.read_to_end(&mut table)?;
    if table.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pruning table length mismatch",
        ));
    }
    Ok(table)
}

#[must_use]
pub fn default_candidates() -> Vec<CandidateSpec> {
    use Component::{
        Corner, E0, E1, E2, E3, Edge3, Edge4, RlCenter, RlCenter2, RlCenter3, UfCenter,
        UfCenter2, UfCenter3,
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
        Corner, E0, E1, E2, E3, Edge3, Edge4, RlCenter, RlCenter2, RlCenter3, UfCenter,
        UfCenter2, UfCenter3,
    };
    [
        Corner, E0, E1, E2, E3, Edge3, Edge4, UfCenter, RlCenter, UfCenter2, UfCenter3,
        RlCenter2, RlCenter3,
    ]
}

#[must_use]
pub fn generated_candidates(max_components: usize, max_entries: usize) -> Vec<CandidateSpec> {
    let components = all_components();
    let mut out = Vec::new();
    for width in 1..=max_components {
        push_combinations(&components, width, 0, &mut Vec::new(), max_entries, &mut out);
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
    use crate::{
        pruning::{evaluate_candidate, CandidateSpec, Component},
        tables::TransitionTables,
    };

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
}
