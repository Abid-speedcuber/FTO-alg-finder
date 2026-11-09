//! Persistent, memory-bounded near-goal membership tables for full and last-layer search.
use crate::{
    FtoCoord, FtoCubie, coord,
    moves::{MOVE_COUNT, Move, move_cubies},
    tables::TransitionTables,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const MAGIC: &[u8; 16] = b"FTO_BLOOM_V1\0\0\0\0";
const HEADER_BYTES: u64 = 72;
const HASHES: u32 = 7;
const MAX_FPR: f64 = 0.01;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct BloomState {
    pub corner: u16,
    pub edge0: u16,
    pub edge1: u16,
    pub edge3: u16,
    pub uf_center: u32,
    pub uf3: u16,
    pub rl_center: u32,
}
impl From<FtoCoord> for BloomState {
    fn from(s: FtoCoord) -> Self {
        Self {
            corner: s.corner,
            edge0: s.edge.e0,
            edge1: s.edge.e1,
            edge3: s.edge3,
            uf_center: s.uf_center,
            uf3: s.uf_center3,
            rl_center: s.rl_center,
        }
    }
}
impl BloomState {
    fn step(self, t: &TransitionTables, m: Move) -> Self {
        Self {
            corner: t.corner_move(self.corner, m),
            edge0: t.edge_choice_move(self.edge0, m),
            edge1: t.edge_choice_move(self.edge1, m),
            edge3: t.edge3_move(self.edge3, m),
            uf_center: t.uf_center_move(self.uf_center, m),
            uf3: t.uf_center3_move(self.uf3, m),
            rl_center: t.rl_center_move(self.rl_center, m),
        }
    }
    fn hashes(self) -> (u64, u64) {
        let a = u64::from(self.corner)
            | (u64::from(self.edge0) << 16)
            | (u64::from(self.edge1) << 32)
            | (u64::from(self.edge3) << 48);
        let b = u64::from(self.uf_center)
            | (u64::from(self.rl_center) << 20)
            | (u64::from(self.uf3) << 40);
        let h = mix(a ^ mix(b ^ 0x9e3779b97f4a7c15));
        (h, mix(h ^ 0xd1b54a32d192ed03) | 1)
    }
}
#[inline]
fn probe_bit(hash: u64, bits: u64) -> usize {
    if bits.is_power_of_two() {
        (hash & (bits - 1)) as usize
    } else {
        (hash % bits) as usize
    }
}
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn digest_step(h: u64, value: u64) -> u64 {
    (h ^ value).wrapping_mul(0x100000001b3)
}
fn model_signature() -> u64 {
    static SIGNATURE: OnceLock<u64> = OnceLock::new();
    *SIGNATURE.get_or_init(|| {
        let mut h = 0xcbf29ce484222325;
        for n in [
            coord::CORNER_COUNT,
            coord::EDGE_CHOICE_COUNT,
            coord::EDGE3_COUNT,
            coord::CENTER_COUNT,
            coord::CENTER3_COUNT,
        ] {
            h = digest_step(h, n as u64);
        }
        for cubie in std::iter::once(FtoCubie::solved()).chain(move_cubies()) {
            for bytes in [
                &cubie.cp[..],
                &cubie.co[..],
                &cubie.ep[..],
                &cubie.uf[..],
                &cubie.rl[..],
            ] {
                for &b in bytes {
                    h = digest_step(h, u64::from(b));
                }
            }
            let (a, b) = BloomState::from(cubie.coord()).hashes();
            h = digest_step(digest_step(h, a), b);
        }
        h
    })
}
fn mask(moves: &[Move]) -> u64 {
    moves.iter().fold(0, |bits, m| bits | (1_u64 << m.idx()))
}
fn active(cancel: Option<&AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err("Bloom table generation cancelled".into())
    } else {
        Ok(())
    }
}

/// Cache goal semantics. Full-position and last-layer tables are never interchangeable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BloomMode {
    FullPosition,
    LastLayer,
}
impl BloomMode {
    fn from_header(value: u64) -> Result<Self, String> {
        match value >> 8 {
            0 => Ok(Self::FullPosition),
            1 => Ok(Self::LastLayer),
            _ => Err("unsupported Bloom goal mode".into()),
        }
    }
    fn flag(self) -> u64 {
        if self == Self::LastLayer { 1 << 8 } else { 0 }
    }
}

pub(crate) fn last_layer_hashes(a: u64, b: u64, c: u64) -> (u64, u64) {
    let h = mix(a ^ mix(b ^ mix(c ^ 0x4c4c5f424c4f4f4d)));
    (h, mix(h ^ 0xd1b54a32d192ed03) | 1)
}
fn signature_for_mode(mode: BloomMode) -> u64 {
    if mode == BloomMode::FullPosition {
        return model_signature();
    }
    static LL_SIGNATURE: OnceLock<u64> = OnceLock::new();
    *LL_SIGNATURE.get_or_init(|| {
        let mut h = digest_step(model_signature(), 0x4c4c5f5631);
        for cubie in std::iter::once(FtoCubie::solved()).chain(move_cubies()) {
            let (a, b) = crate::search::LastLayerState::from_cubie(cubie).bloom_hashes();
            h = digest_step(digest_step(h, a), b);
        }
        h
    })
}

#[derive(Clone, Copy)]
enum BuildState {
    Full(BloomState),
    LastLayer(crate::search::LastLayerState),
}
impl BuildState {
    #[cfg(feature = "gpu")]
    fn gpu_words(self) -> [u32; 10] {
        match self {
            Self::Full(s) => [
                s.corner.into(),
                s.edge0.into(),
                s.edge1.into(),
                s.edge3.into(),
                s.uf_center,
                s.uf3.into(),
                s.rl_center,
                0,
                0,
                0,
            ],
            Self::LastLayer(s) => s.bloom_gpu_words(),
        }
    }
    fn hashes(self) -> (u64, u64) {
        match self {
            Self::Full(s) => s.hashes(),
            Self::LastLayer(s) => s.bloom_hashes(),
        }
    }
    fn step(self, tables: &TransitionTables, mv: Move, positions: &[[u8; 12]; MOVE_COUNT]) -> Self {
        match self {
            Self::Full(s) => Self::Full(s.step(tables, mv)),
            Self::LastLayer(s) => Self::LastLayer(s.bloom_step(tables, mv, positions)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BloomInfo {
    pub mode: BloomMode,
    pub depth: u8,
    pub moves: Vec<Move>,
    pub bytes: u64,
    pub paths: u64,
    pub false_positive_rate: f64,
}
#[derive(Clone, Debug)]
pub struct BloomProgress {
    pub visited: u64,
    pub total: u64,
    pub false_positive_rate: f64,
}
pub type BloomReporter<'a> = dyn Fn(&BloomProgress) + Send + Sync + 'a;
#[derive(Clone, Copy, Debug)]
pub struct GenerateConfig {
    pub mode: BloomMode,
    pub depth: u8,
    pub max_memory_mib: u32,
}

pub struct BloomTable {
    mode: BloomMode,
    depth: u8,
    moves: u64,
    words: Vec<u64>,
    occupied: u64,
    paths: u64,
}
impl std::fmt::Debug for BloomTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BloomTable")
            .field("info", &self.info())
            .finish()
    }
}
impl BloomTable {
    pub fn mode(&self) -> BloomMode {
        self.mode
    }
    pub fn depth(&self) -> u8 {
        self.depth
    }
    pub fn covers(&self, moves: &[Move]) -> bool {
        !moves.is_empty() && mask(moves) & !self.moves == 0
    }
    pub fn info(&self) -> BloomInfo {
        BloomInfo {
            mode: self.mode,
            depth: self.depth,
            moves: Move::ALL
                .into_iter()
                .filter(|m| self.moves & (1 << m.idx()) != 0)
                .collect(),
            bytes: HEADER_BYTES + self.words.len() as u64 * 8,
            paths: self.paths,
            false_positive_rate: self.false_positive_rate(),
        }
    }
    pub fn false_positive_rate(&self) -> f64 {
        (self.occupied as f64 / (self.words.len() as f64 * 64.)).powi(HASHES as i32)
    }
    pub(crate) fn contains(&self, state: BloomState) -> bool {
        self.contains_hashes(state.hashes())
    }
    pub(crate) fn contains_hashes(&self, (a, b): (u64, u64)) -> bool {
        let bits = self.words.len() as u64 * 64;
        (0..HASHES).all(|i| {
            let bit = probe_bit(a.wrapping_add(u64::from(i).wrapping_mul(b)), bits);
            self.words[bit / 64] & (1 << (bit % 64)) != 0
        })
    }
    #[cfg(test)]
    fn insert(&mut self, state: BloomState) {
        self.insert_hashes(state.hashes());
    }
    fn insert_hashes(&mut self, (a, b): (u64, u64)) {
        let bits = self.words.len() as u64 * 64;
        for i in 0..HASHES {
            let bit = probe_bit(a.wrapping_add(u64::from(i).wrapping_mul(b)), bits);
            let flag = 1 << (bit % 64);
            let word = &mut self.words[bit / 64];
            if *word & flag == 0 {
                *word |= flag;
                self.occupied += 1;
            }
        }
    }
    fn shrink(&mut self, cancel: Option<&AtomicBool>) -> Result<(), String> {
        while self.words.len() > 1 && self.words.len() % 2 == 0 {
            active(cancel)?;
            let half = self.words.len() / 2;
            let mut occupied = 0_u64;
            for i in 0..half {
                if i & 8191 == 0 {
                    active(cancel)?;
                }
                occupied += u64::from((self.words[i] | self.words[i + half]).count_ones());
            }
            if (occupied as f64 / (half as f64 * 64.)).powi(HASHES as i32) > MAX_FPR {
                break;
            }
            for i in 0..half {
                if i & 8191 == 0 {
                    active(cancel)?;
                }
                self.words[i] |= self.words[i + half];
            }
            self.words.truncate(half);
            self.occupied = occupied;
        }
        self.words.shrink_to_fit();
        Ok(())
    }
    pub fn generate(
        tables: &TransitionTables,
        moves: &[Move],
        config: GenerateConfig,
        cancel: Option<&AtomicBool>,
        report: Option<&BloomReporter<'_>>,
    ) -> Result<Self, String> {
        #[cfg(feature = "gpu")]
        if crate::acceleration::enabled() {
            match Self::generate_inner(tables, moves, config, cancel, report, true) {
                Ok(table) => return Ok(table),
                Err(error) if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) => {
                    return Err(error);
                }
                Err(error) if error.starts_with("GPU Bloom:") => {
                    crate::acceleration::log(&format!(
                        "{error}; discarding partial GPU work and rebuilding on CPU"
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        Self::generate_inner(tables, moves, config, cancel, report, false)
    }
    fn generate_inner(
        tables: &TransitionTables,
        moves: &[Move],
        config: GenerateConfig,
        cancel: Option<&AtomicBool>,
        report: Option<&BloomReporter<'_>>,
        _use_gpu: bool,
    ) -> Result<Self, String> {
        if !(1..=20).contains(&config.depth) {
            return Err("Bloom depth must be between 1 and 20".into());
        }
        if moves.is_empty() {
            return Err("select at least one Bloom move".into());
        }
        active(cancel)?;
        let bytes = allocation_bytes(config.max_memory_mib)?;
        let mut reverse: Vec<_> = moves.iter().map(|m| m.inverse()).collect();
        reverse.sort_unstable_by_key(|m| m.idx());
        reverse.dedup();
        let cubies = move_cubies();
        let mut successors = vec![Vec::new(); MOVE_COUNT + 1];
        successors[MOVE_COUNT] = reverse.clone();
        for &last in &reverse {
            successors[last.idx()] = reverse
                .iter()
                .copied()
                .filter(|&next| {
                    if next == last.inverse() {
                        return false;
                    }
                    if last.axis() == next.axis()
                        && reverse.contains(&last.inverse())
                        && cubies[last.idx()].compose(&cubies[last.idx()])
                            == cubies[last.inverse().idx()]
                    {
                        return false;
                    }
                    !(next.idx() < last.idx()
                        && cubies[last.idx()].compose(&cubies[next.idx()])
                            == cubies[next.idx()].compose(&cubies[last.idx()]))
                })
                .collect();
        }
        let mut layer = vec![0_u128; MOVE_COUNT];
        let mut total = 1_u128;
        for d in 1..=config.depth {
            let mut next = vec![0_u128; MOVE_COUNT];
            if d == 1 {
                for &mv in &reverse {
                    next[mv.idx()] = 1;
                }
            } else {
                for &last in &reverse {
                    for &mv in &successors[last.idx()] {
                        next[mv.idx()] = next[mv.idx()]
                            .checked_add(layer[last.idx()])
                            .ok_or("Bloom traversal is too large; reduce the depth or move set")?;
                    }
                }
            }
            total = total
                .checked_add(next.iter().sum())
                .ok_or("Bloom traversal is too large")?;
            layer = next;
        }
        let total = u64::try_from(
            total
                * if config.mode == BloomMode::LastLayer {
                    3
                } else {
                    1
                },
        )
        .map_err(|_| "Bloom traversal is too large; reduce the depth or move set")?;
        // A traversal can insert at most `total` distinct states. Avoid reserving
        // a multi-GiB budget for a tiny job; this upper bound is deliberately
        // conservative and is not a cardinality estimate exposed to the user.
        let needed = (u128::from(total) * 2).next_power_of_two().max(8);
        let bytes = bytes.min(usize::try_from(needed).unwrap_or(usize::MAX));
        crate::acceleration::check_host_allocation(bytes as u64)?;
        let mut words = Vec::new();
        words
            .try_reserve_exact(bytes / 8)
            .map_err(|_| "not enough memory for the Bloom table".to_owned())?;
        words.resize(bytes / 8, 0);
        let mut table = Self {
            mode: config.mode,
            depth: config.depth,
            moves: mask(moves),
            words,
            occupied: 0,
            paths: 0,
        };
        #[cfg(feature = "gpu")]
        let gpu = if _use_gpu
            && total
                >= (if crate::acceleration::forced() {
                    16_384
                } else {
                    1_000_000
                })
            && reverse.len() >= 2
        {
            prepare_gpu(
                tables,
                &reverse,
                &successors,
                config,
                cancel,
                table.words.len() as u64 * 8,
            )
            .map_err(|e| format!("GPU Bloom: {e}"))?
        } else {
            None
        };
        let mut build = Builder {
            table: &mut table,
            tables,
            #[cfg(feature = "gpu")]
            gpu,
            #[cfg(feature = "gpu")]
            gpu_roots: Vec::new(),
            #[cfg(feature = "gpu")]
            gpu_calibrated: false,
            #[cfg(feature = "gpu")]
            reverse: reverse.clone(),
            successors,
            positions: crate::search::last_layer_position_moves(),
            cancel,
            report,
            total,
            last_report: Instant::now(),
        };
        if let Some(report) = report {
            report(&BloomProgress {
                visited: 0,
                total,
                false_positive_rate: 0.,
            });
        }
        match config.mode {
            BloomMode::FullPosition => build.visit(
                BuildState::Full(FtoCoord::solved().into()),
                config.depth,
                MOVE_COUNT,
            )?,
            BloomMode::LastLayer => {
                // Trailing AUF is free, even when U is absent from the selected moves.
                // Leading AUF is already branched by the last-layer search before querying.
                let goal = BuildState::LastLayer(crate::search::LastLayerState::from_cubie(
                    FtoCubie::solved(),
                ));
                for seed in [
                    goal,
                    goal.step(tables, Move::U, &build.positions),
                    goal.step(tables, Move::Up, &build.positions),
                ] {
                    build.visit(seed, config.depth, MOVE_COUNT)?;
                }
            }
        }
        #[cfg(feature = "gpu")]
        build.flush_gpu()?;
        #[cfg(feature = "gpu")]
        build.finish_gpu()?;
        active(cancel)?;
        if table.paths != total {
            return Err("Bloom traversal did not complete".into());
        }
        if table.false_positive_rate() > MAX_FPR {
            return Err(
                "Bloom table is full; choose a smaller depth or a larger memory limit".into(),
            );
        }
        table.shrink(cancel)?;
        if let Some(report) = report {
            report(&BloomProgress {
                visited: total,
                total,
                false_positive_rate: table.false_positive_rate(),
            });
        }
        Ok(table)
    }
}
struct Builder<'a> {
    #[cfg(feature = "gpu")]
    gpu: Option<crate::acceleration::BloomCompute>,
    #[cfg(feature = "gpu")]
    gpu_roots: Vec<(BuildState, usize)>,
    #[cfg(feature = "gpu")]
    gpu_calibrated: bool,
    #[cfg(feature = "gpu")]
    reverse: Vec<Move>,
    table: &'a mut BloomTable,
    tables: &'a TransitionTables,
    successors: Vec<Vec<Move>>,
    positions: [[u8; 12]; MOVE_COUNT],
    cancel: Option<&'a AtomicBool>,
    report: Option<&'a BloomReporter<'a>>,
    total: u64,
    last_report: Instant,
}
impl Builder<'_> {
    #[cfg(feature = "gpu")]
    fn flush_gpu(&mut self) -> Result<(), String> {
        let Some(gpu) = &mut self.gpu else {
            return Ok(());
        };
        let tail_depth = gpu.tail_depth;
        let hashes = gpu
            .flush(self.cancel)
            .map_err(|e| format!("GPU Bloom: {e}"))?;
        let gpu_time = gpu.last_elapsed;
        let gpu_paths = gpu.last_paths;
        let resident = gpu.resident();
        let gpu_full = gpu.false_positive_rate() > MAX_FPR;
        let roots = std::mem::take(&mut self.gpu_roots);
        if !crate::acceleration::forced()
            && !self.gpu_calibrated
            && (hashes.len() >= 16_384 || (hashes.len() >= 1024 && gpu_time.as_millis() >= 16))
        {
            let started = Instant::now();
            let mut sample = Vec::new();
            for &(state, last) in roots.iter().take(64) {
                self.cpu_pilot(state, tail_depth, last, &mut sample)?;
            }
            if resident {
                for &hash in &sample {
                    self.table.insert_hashes(hash);
                }
            }
            let cpu_time = started.elapsed();
            let gpu_rate = gpu_time.as_secs_f64() / hashes.len().max(1) as f64;
            let cpu_rate = cpu_time.as_secs_f64() / sample.len().max(1) as f64;
            let actual: std::collections::HashSet<_> = hashes.iter().copied().collect();
            if sample.iter().any(|h| !actual.contains(h)) {
                return Err("GPU Bloom: pilot hash mismatch".into());
            }
            self.gpu_calibrated = true;
            if gpu_rate > cpu_rate * 0.8 {
                crate::acceleration::log(&format!(
                    "GPU Bloom pilot is slower than CPU ({:.0} vs {:.0} ns/state); continuing on CPU",
                    gpu_rate * 1e9,
                    cpu_rate * 1e9
                ));
                self.gpu = None;
            } else if let Some(gpu) = &mut self.gpu {
                gpu.retain_gpu();
            }
        }
        self.table.paths += gpu_paths;
        for (i, hash) in hashes.into_iter().enumerate() {
            if i & 1023 == 0 {
                active(self.cancel)?;
            }
            self.table.insert_hashes(hash);
        }
        if gpu_full || self.table.false_positive_rate() > MAX_FPR {
            return Err(
                "Bloom table is full; choose a smaller depth or a larger memory limit".into(),
            );
        }
        if self.last_report.elapsed().as_millis() >= 200 {
            if let Some(report) = self.report {
                report(&BloomProgress {
                    visited: self.table.paths,
                    total: self.total,
                    false_positive_rate: self.table.false_positive_rate(),
                });
            }
            self.last_report = Instant::now();
        }
        Ok(())
    }
    #[cfg(feature = "gpu")]
    fn finish_gpu(&mut self) -> Result<(), String> {
        if let Some(gpu) = self.gpu.take() {
            if gpu.resident() {
                gpu.merge_filter(&mut self.table.words, self.cancel)
                    .map_err(|e| format!("GPU Bloom: {e}"))?;
                self.table.occupied = 0;
                for chunk in self.table.words.chunks(8192) {
                    active(self.cancel)?;
                    self.table.occupied +=
                        chunk.iter().map(|w| u64::from(w.count_ones())).sum::<u64>();
                }
            }
        }
        Ok(())
    }
    #[cfg(feature = "gpu")]
    fn cpu_pilot(
        &self,
        state: BuildState,
        left: u8,
        last: usize,
        hashes: &mut Vec<(u64, u64)>,
    ) -> Result<(), String> {
        if hashes.len() & 1023 == 0 {
            active(self.cancel)?;
        }
        hashes.push(state.hashes());
        if left > 0 {
            for &mv in &self.successors[last] {
                self.cpu_pilot(
                    state.step(self.tables, mv, &self.positions),
                    left - 1,
                    mv.idx(),
                    hashes,
                )?;
            }
        }
        Ok(())
    }
    fn visit(&mut self, state: BuildState, left: u8, last: usize) -> Result<(), String> {
        #[cfg(feature = "gpu")]
        if self.gpu.as_ref().is_some_and(|gpu| left == gpu.tail_depth) {
            let mut job = [0_u32; 11];
            job[..10].copy_from_slice(&state.gpu_words());
            job[10] = self
                .reverse
                .iter()
                .position(|m| m.idx() == last)
                .unwrap_or(self.reverse.len()) as u32;
            self.gpu_roots.push((state, last));
            let gpu = self.gpu.as_mut().unwrap();
            gpu.pending.push(job);
            if gpu.ready() {
                self.flush_gpu()?;
            }
            return Ok(());
        }
        self.table.insert_hashes(state.hashes());
        self.table.paths += 1;
        if self.table.paths & 1023 == 0 {
            active(self.cancel)?;
            if self.table.false_positive_rate() > MAX_FPR {
                return Err(
                    "Bloom table is full; choose a smaller depth or a larger memory limit".into(),
                );
            }
            if self.last_report.elapsed().as_millis() >= 200 {
                if let Some(report) = self.report {
                    report(&BloomProgress {
                        visited: self.table.paths,
                        total: self.total,
                        false_positive_rate: self.table.false_positive_rate(),
                    });
                }
                self.last_report = Instant::now();
            }
        }
        if left == 0 {
            return Ok(());
        }
        for i in 0..self.successors[last].len() {
            let mv = self.successors[last][i];
            self.visit(
                state.step(self.tables, mv, &self.positions),
                left - 1,
                mv.idx(),
            )?;
        }
        Ok(())
    }
}

#[cfg(feature = "gpu")]
fn prepare_gpu(
    tables: &TransitionTables,
    reverse: &[Move],
    successors: &[Vec<Move>],
    config: GenerateConfig,
    cancel: Option<&AtomicBool>,
    filter_bytes: u64,
) -> Result<Option<crate::acceleration::BloomCompute>, String> {
    use crate::pruning::Component;
    let Some(gpu) = crate::acceleration::gpu() else {
        return Ok(None);
    };
    let components = if config.mode == BloomMode::LastLayer {
        [
            Component::Corner,
            Component::E0,
            Component::E0,
            Component::Edge3,
            Component::UfCenter3,
            Component::UfCenter3,
            Component::RlCenter,
        ]
    } else {
        [
            Component::Corner,
            Component::E0,
            Component::E0,
            Component::Edge3,
            Component::UfCenter,
            Component::UfCenter3,
            Component::RlCenter,
        ]
    };
    let mut parameters = vec![0; 32 + (reverse.len() + 1) * 2];
    parameters[0] = u32::from(config.mode == BloomMode::LastLayer);
    parameters[1] = reverse.len() as u32;
    let mut transitions = Vec::new();
    let mut known = Vec::new();
    for (slot, component) in components.into_iter().enumerate() {
        let offset = if let Some(&(_, offset)) = known.iter().find(|&&(c, _)| c == component) {
            offset
        } else {
            let offset =
                u32::try_from(transitions.len()).map_err(|_| "GPU transition address overflow")?;
            let count = component.size() * reverse.len();
            if (transitions.len() + count) as u64 * 4 > gpu.binding_limit {
                return Ok(None);
            }
            transitions
                .try_reserve_exact(count)
                .map_err(|_| "not enough RAM for GPU transitions")?;
            for value in 0..component.size() {
                if value & 1023 == 0 {
                    active(cancel)?;
                }
                for &mv in reverse {
                    transitions.push(component.next(value as u32, mv, tables));
                }
            }
            known.push((component, offset));
            offset
        };
        parameters[8 + slot] = offset;
    }
    parameters[15] = transitions.len() as u32;
    let positions = crate::search::last_layer_position_moves();
    for &mv in reverse {
        transitions.extend(positions[mv.idx()].map(u32::from));
    }
    for last in 0..=reverse.len() {
        let global_last = reverse.get(last).map_or(MOVE_COUNT, |m| m.idx());
        for (next, mv) in reverse.iter().enumerate() {
            if successors[global_last].contains(mv) {
                parameters[32 + last * 2 + next / 32] |= 1 << (next % 32);
            }
        }
    }
    crate::acceleration::BloomCompute::new(
        gpu,
        &transitions,
        parameters,
        config.depth,
        filter_bytes,
    )
}

fn allocation_bytes(memory_mib: u32) -> Result<usize, String> {
    if memory_mib == 0 {
        return Err("Bloom memory budget must be at least 1 MiB".into());
    }
    let bytes = u64::from(memory_mib)
        .checked_mul(1024 * 1024)
        .ok_or("Bloom memory budget overflow")?;
    let bytes = usize::try_from(bytes)
        .map_err(|_| "Bloom memory budget exceeds this platform's address space")?;
    if bytes > isize::MAX as usize {
        return Err("Bloom memory budget exceeds this platform's address space".into());
    }
    Ok(bytes)
}

pub fn cache_file_name(moves: &[Move], depth: u8, mode: BloomMode) -> String {
    let prefix = if mode == BloomMode::LastLayer {
        "ll-"
    } else {
        ""
    };
    format!("{prefix}m{:010x}-d{depth}.bloom", mask(moves))
}

impl BloomTable {
    fn header(&self) -> [u64; 6] {
        [
            signature_for_mode(self.mode),
            self.moves,
            u64::from(self.depth) | self.mode.flag(),
            self.words.len() as u64,
            self.occupied,
            self.paths,
        ]
    }
    pub fn save(&self, path: &Path, cancel: Option<&AtomicBool>) -> Result<(), String> {
        active(cancel)?;
        let parent = path.parent().ok_or("Bloom path has no parent")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let temp = parent.join(format!(".bloom-{}-{nonce}.tmp", std::process::id()));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(temp.clone());
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let mut writer = BufWriter::new(file);
        writer.write_all(MAGIC).map_err(|e| e.to_string())?;
        let mut checksum = self
            .header()
            .into_iter()
            .fold(0xcbf29ce484222325, digest_step);
        for chunk in self.words.chunks(8192) {
            active(cancel)?;
            for &word in chunk {
                checksum = digest_step(checksum, word);
            }
        }
        for n in self.header().into_iter().chain([checksum]) {
            writer
                .write_all(&n.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        let mut buffer = Vec::with_capacity(64 * 1024);
        for chunk in self.words.chunks(8192) {
            active(cancel)?;
            buffer.clear();
            for word in chunk {
                buffer.extend_from_slice(&word.to_le_bytes());
            }
            writer.write_all(&buffer).map_err(|e| e.to_string())?;
        }
        writer.flush().map_err(|e| e.to_string())?;
        writer.get_ref().sync_all().map_err(|e| e.to_string())?;
        drop(writer);
        active(cancel)?;
        match fs::rename(&temp, path) {
            Ok(()) => Ok(()),
            Err(first) if path.exists() => {
                let backup = parent.join(format!(".bloom-{}-{nonce}.previous", std::process::id()));
                fs::rename(path, &backup).map_err(|_| first.to_string())?;
                match fs::rename(&temp, path) {
                    Ok(()) => {
                        let _ = fs::remove_file(backup);
                        Ok(())
                    }
                    Err(error) => {
                        let _ = fs::rename(backup, path);
                        Err(error.to_string())
                    }
                }
            }
            Err(error) => Err(error.to_string()),
        }
    }
    pub fn read_info(path: &Path) -> Result<BloomInfo, String> {
        let mut reader = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
        let (header, _) = read_header(&mut reader, path)?;
        Ok(info_from_header(&header))
    }
    pub fn load(path: &Path, cancel: Option<&AtomicBool>) -> Result<Self, String> {
        active(cancel)?;
        let mut reader = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
        let (header, expected) = read_header(&mut reader, path)?;
        crate::acceleration::check_host_allocation(header[3] * 8)?;
        let count = header[3] as usize;
        let mut words = Vec::new();
        words
            .try_reserve_exact(count)
            .map_err(|_| "not enough memory to load the Bloom table".to_owned())?;
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut remaining = count;
        let mut checksum = header.into_iter().fold(0xcbf29ce484222325, digest_step);
        let mut occupied = 0_u64;
        while remaining > 0 {
            active(cancel)?;
            let n = remaining.min(buffer.len() / 8);
            reader
                .read_exact(&mut buffer[..n * 8])
                .map_err(|e| e.to_string())?;
            for bytes in buffer[..n * 8].chunks_exact(8) {
                let word = u64::from_le_bytes(bytes.try_into().unwrap());
                checksum = digest_step(checksum, word);
                occupied += u64::from(word.count_ones());
                words.push(word);
            }
            remaining -= n;
        }
        let table = Self {
            mode: BloomMode::from_header(header[2])?,
            depth: header[2] as u8,
            moves: header[1],
            words,
            occupied: header[4],
            paths: header[5],
        };
        if checksum != expected || occupied != table.occupied {
            return Err("Bloom table integrity check failed; regenerate this table".into());
        }
        active(cancel)?;
        Ok(table)
    }
}
fn read_header(reader: &mut impl Read, path: &Path) -> Result<([u64; 6], u64), String> {
    let mut magic = [0; 16];
    reader.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != MAGIC {
        return Err("unsupported Bloom table format".into());
    }
    let mut numbers = [0_u64; 7];
    for value in &mut numbers {
        let mut bytes = [0; 8];
        reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        *value = u64::from_le_bytes(bytes);
    }
    let header: [u64; 6] = numbers[..6].try_into().unwrap();
    if header[0] != signature_for_mode(BloomMode::from_header(header[2])?) {
        return Err("Bloom table belongs to a different solver version".into());
    }
    if header[1] == 0
        || header[1] >> MOVE_COUNT != 0
        || !(1..=20).contains(&(header[2] & 255))
        || header[3] == 0
        || header[3] > isize::MAX as u64 / 8
        || header[4] > header[3] * 64
        || header[5] == 0
    {
        return Err("invalid Bloom table header".into());
    }
    let length = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if length != HEADER_BYTES + header[3] * 8 {
        return Err("Bloom table is truncated or has invalid length".into());
    }
    Ok((header, numbers[6]))
}
fn info_from_header(h: &[u64; 6]) -> BloomInfo {
    BloomInfo {
        mode: BloomMode::from_header(h[2]).expect("validated mode"),
        depth: h[2] as u8,
        moves: Move::ALL
            .into_iter()
            .filter(|m| h[1] & (1 << m.idx()) != 0)
            .collect(),
        bytes: HEADER_BYTES + h[3] * 8,
        paths: h[5],
        false_positive_rate: (h[4] as f64 / (h[3] as f64 * 64.)).powi(HASHES as i32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, sync::Arc};

    fn sample() -> (BloomTable, Vec<BloomState>) {
        let mut table = BloomTable {
            mode: BloomMode::FullPosition,
            depth: 3,
            moves: mask(&[Move::R, Move::Rp]),
            words: vec![0; 4096],
            occupied: 0,
            paths: 200,
        };
        let states: Vec<_> = (0..200)
            .map(|i| BloomState {
                corner: i,
                edge0: i * 3,
                edge1: i * 7,
                edge3: i * 11,
                uf_center: i as u32 * 101,
                uf3: i * 13,
                rl_center: i as u32 * 103,
            })
            .collect();
        for &state in &states {
            table.insert(state);
        }
        (table, states)
    }

    #[test]
    fn multi_gib_budgets_and_non_power_of_two_membership() {
        if usize::BITS >= 64 {
            assert_eq!(
                allocation_bytes(12 * 1024).unwrap() as u64,
                12 * 1024 * 1024 * 1024
            );
        }
        assert!(allocation_bytes(0).is_err());
        let (mut table, states) = sample();
        table.words = vec![0; 3072];
        table.occupied = 0;
        for &state in &states {
            table.insert(state);
        }
        table.shrink(None).unwrap();
        assert!(states.iter().all(|&s| table.contains(s)));
        assert!(table.false_positive_rate() <= MAX_FPR);
    }

    #[cfg(unix)]
    #[test]
    fn metadata_accepts_twelve_gib_without_allocating_the_filter() {
        let (table, _) = sample();
        let root = std::env::temp_dir().join(format!(
            "fto-large-bloom-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("large.bloom");
        let mut header = table.header();
        let bytes = 12_u64 * 1024 * 1024 * 1024;
        header[3] = bytes / 8;
        header[4] = 0;
        let mut file = File::create(&path).unwrap();
        file.write_all(MAGIC).unwrap();
        for word in header.into_iter().chain([0]) {
            file.write_all(&word.to_le_bytes()).unwrap();
        }
        file.set_len(HEADER_BYTES + bytes).unwrap();
        drop(file);
        let info = BloomTable::read_info(&path).unwrap();
        assert_eq!(info.bytes, bytes + HEADER_BYTES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folding_and_persistent_cache_preserve_membership() {
        for word_count in [4096, 3072] {
            let (mut table, states) = sample();
            table.words = vec![0; word_count];
            table.occupied = 0;
            for &state in &states {
                table.insert(state);
            }
            table.shrink(None).unwrap();
            assert!(table.words.len() < 4096);
            assert!(table.false_positive_rate() <= MAX_FPR);
            assert!(states.iter().all(|&s| table.contains(s)));
            assert!(table.covers(&[Move::R]));
            assert!(!table.covers(&[Move::U]));
            let root = std::env::temp_dir().join(format!(
                "fto-bloom-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let path = root.join(cache_file_name(
                &table.info().moves,
                table.depth,
                table.mode,
            ));
            table.save(&path, None).unwrap();
            let original = fs::read(&path).unwrap();
            let loaded = BloomTable::load(&path, None).unwrap();
            assert!(states.iter().all(|&s| loaded.contains(s)));
            assert_eq!(
                BloomTable::read_info(&path).unwrap().bytes,
                original.len() as u64
            );
            let cancel = AtomicBool::new(true);
            assert!(table.save(&path, Some(&cancel)).is_err());
            assert_eq!(original, fs::read(&path).unwrap());
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            assert!(BloomTable::load(&path, Some(&cancel)).is_err());
            for offset in [24, 32, 72] {
                // moves, depth and payload corruption
                let mut corrupt = original.clone();
                corrupt[offset] ^= 1;
                fs::write(&path, corrupt).unwrap();
                assert!(BloomTable::load(&path, None).is_err());
            }
            fs::write(&path, &original[..original.len() - 1]).unwrap();
            assert!(BloomTable::load(&path, None).is_err());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires GPU hardware"]
    fn gpu_resident_bloom_non_power_of_two_bits_match_cpu() {
        use crate::acceleration::{self, Policy};
        acceleration::with_policy(Policy::Gpu, None, || {
            let gpu = acceleration::gpu().expect("GPU hardware required");
            let mut transitions = Vec::new();
            for value in 0..512_u32 {
                transitions.extend([value, value]);
            }
            let mut parameters = vec![0; 38];
            parameters[1] = 2;
            for last in 0..3 {
                parameters[32 + last * 2] = 3;
            }
            for word_count in [1024, 1536] {
                let mut engine = acceleration::BloomCompute::new(
                    gpu.clone(),
                    &transitions,
                    parameters.clone(),
                    2,
                    word_count as u64 * 8,
                )
                .unwrap()
                .unwrap();
                assert!(engine.resident());
                let mut table = BloomTable {
                    mode: BloomMode::FullPosition,
                    depth: 2,
                    moves: mask(&[Move::R, Move::Rp]),
                    words: vec![0; word_count],
                    occupied: 0,
                    paths: 1,
                };
                for n in 0..300_u16 {
                    let state = BloomState {
                        corner: n,
                        edge0: n * 7 % 512,
                        edge1: n * 13 % 512,
                        edge3: n * 17 % 512,
                        uf_center: u32::from(n) * 23 % 512,
                        uf3: n * 31 % 512,
                        rl_center: u32::from(n) * 37 % 512,
                    };
                    let mut job = [0_u32; 11];
                    job[..10].copy_from_slice(&BuildState::Full(state).gpu_words());
                    job[10] = 2;
                    engine.pending.push(job);
                    table.insert(state);
                    if engine.ready() {
                        assert!(engine.flush(None).unwrap().is_empty());
                    }
                }
                engine.flush(None).unwrap();
                let mut actual = vec![0_u64; word_count];
                engine.merge_filter(&mut actual, None).unwrap();
                assert_eq!(
                    table.words, actual,
                    "resident GPU modulo mismatch for {word_count} words"
                );
                assert_eq!(table.occupied, engine.occupied);
                assert!(engine.flush(None).unwrap().is_empty());
                assert_eq!(engine.last_paths, 0);
            }
        });
    }

    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires real transition cache and GPU hardware"]
    fn gpu_bloom_matches_cpu_for_full_last_layer_and_all_moves() {
        use crate::acceleration::{self, Policy};
        let tables = TransitionTables::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../cache/transition-table.tran"
        ))
        .unwrap();
        let cases = [
            (
                BloomMode::FullPosition,
                vec![Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp],
                7,
            ),
            (
                BloomMode::LastLayer,
                vec![Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp],
                6,
            ),
            (BloomMode::FullPosition, Move::ALL.to_vec(), 3),
        ];
        for (mode, moves, depth) in cases {
            let config = GenerateConfig {
                mode,
                depth,
                max_memory_mib: 3,
            };
            let started = Instant::now();
            let cpu = BloomTable::generate(&tables, &moves, config, None, None).unwrap();
            let cpu_time = started.elapsed();
            let messages = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let sink = messages.clone();
            let log: Arc<crate::acceleration::GenerationLog> = Arc::new(move |s| {
                println!("{s}");
                sink.lock().unwrap().push(s.into());
            });
            let started = Instant::now();
            let gpu = acceleration::with_policy(Policy::Gpu, Some(log), || {
                BloomTable::generate(&tables, &moves, config, None, None)
            })
            .unwrap();
            assert!(
                messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|s| s.contains("Bloom GPU enumeration")),
                "no GPU enumeration executed"
            );
            assert!(
                !messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|s| s.contains("rebuilding")),
                "GPU failed and silently fell back"
            );
            assert_eq!(cpu.words, gpu.words, "GPU Bloom bit mismatch for {mode:?}");
            assert_eq!(cpu.paths, gpu.paths);
            println!(
                "GPU BLOOM PARITY {mode:?} moves={} depth={depth} paths={} CPU={cpu_time:?} GPU={:?}",
                moves.len(),
                gpu.paths,
                started.elapsed()
            );
        }
        // Accept a 12-GiB budget without allocating it for a tiny traversal.
        let huge_budget = BloomTable::generate(
            &tables,
            &[Move::R],
            GenerateConfig {
                mode: BloomMode::LastLayer,
                depth: 4,
                max_memory_mib: 12 * 1024,
            },
            None,
            None,
        )
        .unwrap();
        assert!(huge_budget.info().bytes < 1024 * 1024);
    }

    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires real transition cache and GPU hardware"]
    fn gpu_auto_bloom_pilot_preserves_results_when_switching_to_cpu() {
        use crate::acceleration::{self, Policy};
        let tables = TransitionTables::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../cache/transition-table.tran"
        ))
        .unwrap();
        let moves = [Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp];
        let config = GenerateConfig {
            mode: BloomMode::LastLayer,
            depth: 9,
            max_memory_mib: 16,
        };
        let expected = BloomTable::generate(&tables, &moves, config, None, None).unwrap();
        let messages = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = messages.clone();
        let log: Arc<acceleration::GenerationLog> = Arc::new(move |message| {
            println!("{message}");
            sink.lock().unwrap().push(message.into());
        });
        let actual = acceleration::with_policy(Policy::Auto, Some(log), || {
            BloomTable::generate(&tables, &moves, config, None, None)
        })
        .unwrap();
        assert!(
            messages
                .lock()
                .unwrap()
                .iter()
                .any(|s| s.contains("Bloom GPU enumeration"))
        );
        assert_eq!(expected.words, actual.words);
        assert_eq!(expected.paths, actual.paths);
    }

    #[test]
    #[ignore = "loads the real cached transition tables"]
    fn generated_radius_and_threaded_search_match_exact_search() {
        let tables = TransitionTables::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../cache/transition-table.tran"
        ))
        .unwrap();
        let sets = vec![
            vec![Move::R],
            vec![Move::R, Move::RURp, Move::RUpRp, Move::U],
            vec![Move::Rw, Move::Rwp, Move::F, Move::Fp],
            Move::ALL.to_vec(),
        ];
        // Independent unpruned BFS checks every reachable state, including all
        // prefixes and repeated one-way moves. No generator reductions reused.
        for moves in sets {
            let depth = if moves.len() > 8 { 3 } else { 5 };
            let table = BloomTable::generate(
                &tables,
                &moves,
                GenerateConfig {
                    mode: BloomMode::FullPosition,
                    depth,
                    max_memory_mib: 4,
                },
                None,
                None,
            )
            .unwrap();
            let mut seen = HashSet::from([BloomState::from(FtoCoord::solved())]);
            let mut frontier = seen.clone();
            for _ in 0..depth {
                let mut next = HashSet::new();
                for &state in &frontier {
                    for &mv in &moves {
                        let child = state.step(&tables, mv.inverse());
                        if seen.insert(child) {
                            next.insert(child);
                        }
                    }
                }
                frontier = next;
            }
            assert!(
                seen.iter().all(|&s| table.contains(s)),
                "missing a reachable state for {moves:?}"
            );
        }
        let moves = vec![Move::R, Move::Rp, Move::U, Move::Up, Move::F, Move::Fp];
        let start = [Move::R, Move::U, Move::F, Move::Rp, Move::U]
            .into_iter()
            .fold(FtoCubie::solved(), |s, m| s.apply(m))
            .coord();
        for depth in [2, 5] {
            // exercise both worker DFS and root-collection checks
            let table = Arc::new(
                BloomTable::generate(
                    &tables,
                    &moves,
                    GenerateConfig {
                        mode: BloomMode::FullPosition,
                        depth,
                        max_memory_mib: 1,
                    },
                    None,
                    None,
                )
                .unwrap(),
            );
            let base = crate::search::SearchConfig {
                min_depth: 5,
                max_depth: 5,
                allowed_moves: moves.clone(),
                ..Default::default()
            };
            let expected = crate::search::solve(start, &tables, &base);
            assert!(!expected.solutions.is_empty());
            for threads in [1, 4] {
                let mut config = base.clone();
                config.bloom = Some(table.clone());
                let mut actual = crate::search::solve_with_pruning_threads(
                    start, &tables, None, &config, threads,
                );
                let mut solutions = expected.solutions.clone();
                solutions.sort_by_key(|s| s.iter().map(|m| m.idx()).collect::<Vec<_>>());
                actual
                    .solutions
                    .sort_by_key(|s| s.iter().map(|m| m.idx()).collect::<Vec<_>>());
                assert_eq!(solutions, actual.solutions);
                if depth == 2 {
                    assert!(actual.nodes < expected.nodes, "Bloom filter did not prune");
                }
            }
        }
        let table = Arc::new(
            BloomTable::generate(
                &tables,
                &moves,
                GenerateConfig {
                    mode: BloomMode::FullPosition,
                    depth: 2,
                    max_memory_mib: 1,
                },
                None,
                None,
            )
            .unwrap(),
        );
        assert!(!table.contains(start.into()));
        let short = crate::search::SearchConfig {
            min_depth: 2,
            max_depth: 2,
            allowed_moves: moves.clone(),
            ..Default::default()
        };
        assert!(
            crate::search::solve(start, &tables, &short)
                .solutions
                .is_empty()
        );
        let mut enabled = short.clone();
        enabled.bloom = Some(table.clone());
        for threads in [1, 4] {
            let result =
                crate::search::solve_with_pruning_threads(start, &tables, None, &enabled, threads);
            assert!(result.solutions.is_empty());
            assert_eq!(result.nodes, 1, "root boundary did not reject the position");
        }
        for free_u in [false, true] {
            let mut base = crate::search::SearchConfig {
                min_depth: 4,
                max_depth: 4,
                allowed_moves: moves.clone(),
                free_u_ends: free_u,
                ..Default::default()
            };
            if !free_u {
                base.allowed_moves.push(Move::B);
            }
            let expected = crate::search::solve(start, &tables, &base);
            base.bloom = Some(table.clone());
            assert_eq!(
                expected,
                crate::search::solve(start, &tables, &base),
                "incompatible goal or move set must skip Bloom pruning"
            );
        }
        let cancel = AtomicBool::new(false);
        let report = |_: &BloomProgress| {
            cancel.store(true, Ordering::Relaxed);
        };
        assert!(
            BloomTable::generate(
                &tables,
                &moves,
                GenerateConfig {
                    mode: BloomMode::FullPosition,
                    depth: 5,
                    max_memory_mib: 1
                },
                Some(&cancel),
                Some(&report)
            )
            .is_err()
        );
        cancel.store(false, Ordering::Relaxed);
        assert!(
            BloomTable::generate(
                &tables,
                &Move::ALL,
                GenerateConfig {
                    mode: BloomMode::FullPosition,
                    depth: 8,
                    max_memory_mib: 1
                },
                Some(&cancel),
                None
            )
            .unwrap_err()
            .contains("full")
        );
    }
}
