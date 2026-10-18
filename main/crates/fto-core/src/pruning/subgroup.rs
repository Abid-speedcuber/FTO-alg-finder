//! Restricted coordinate orbits with compact indices precomputed in move tables.
//! Search never ranks a state or searches sparse storage to read a heuristic.
use super::*;
use std::io::{Seek, SeekFrom};

const OUTSIDE: u16 = u16::MAX;

#[derive(Clone, Debug)]
struct Orbit {
    original: Box<[u16]>,
    rank: Box<[u16]>,
}

impl Orbit {
    fn build(component: Component, root: u16, tables: &TransitionTables, moves: &[Move]) -> Self {
        let mut rank = vec![OUTSIDE; component.size()];
        let mut original = vec![root];
        rank[usize::from(root)] = 0;
        let mut head = 0;
        while head < original.len() {
            let value = original[head];
            head += 1;
            for &mv in moves {
                let next = component.next(u32::from(value), mv, tables) as u16;
                if rank[usize::from(next)] == OUTSIDE {
                    rank[usize::from(next)] = original.len() as u16;
                    original.push(next);
                }
            }
        }
        Self {
            original: original.into_boxed_slice(),
            rank: rank.into_boxed_slice(),
        }
    }
    fn bytes(&self) -> usize {
        size_of_val(&*self.original) + size_of_val(&*self.rank)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Edge,
    Corner,
}
impl Kind {
    fn spec(self) -> CandidateSpec {
        CandidateSpec::new(vec![
            match self {
                Self::Edge => Component::Edge3,
                Self::Corner => Component::Corner,
            },
            Component::UfCenter3,
        ])
    }
}

#[derive(Clone, Debug)]
struct PackedMoves {
    corner: Box<[u64]>,
    edge: Box<[u64]>,
    uf: Box<[u64]>,
    slots: [u8; MOVE_COUNT],
    bits: usize,
}

impl PackedMoves {
    fn build(
        corner: &Orbit,
        edge: &Orbit,
        uf: &Orbit,
        uf_bits: usize,
        moves: &[Move],
        next: impl Fn(Component, u16, Move) -> u16,
    ) -> Self {
        let mut cached_moves = moves.to_vec();
        // Free AUF roots may apply U even when it is not an allowed search move.
        for mv in [Move::U, Move::Up] {
            if !cached_moves.contains(&mv) {
                cached_moves.push(mv);
            }
        }
        let width = cached_moves.len().next_power_of_two();
        let bits = width.trailing_zeros() as usize;
        let mut slots = [u8::MAX; MOVE_COUNT];
        for (slot, mv) in cached_moves.iter().enumerate() {
            slots[mv.idx()] = slot as u8;
        }
        let build = |component: Component, orbit: &Orbit, is_uf: bool| {
            let mut data = vec![0_u64; component.size() * width];
            for value in 0..component.size() {
                for (slot, &mv) in cached_moves.iter().enumerate() {
                    let original_next = next(component, value as u16, mv);
                    let rank = orbit.rank[usize::from(original_next)];
                    let part = if is_uf {
                        if rank == OUTSIDE {
                            (1 << uf_bits) - 1
                        } else {
                            usize::from(rank)
                        }
                    } else {
                        let row = if rank == OUTSIDE {
                            orbit.original.len()
                        } else {
                            usize::from(rank)
                        };
                        row << uf_bits
                    };
                    data[(value << bits) | slot] = u64::from(original_next) | ((part as u64) << 16);
                }
            }
            data.into_boxed_slice()
        };
        Self {
            corner: build(Component::Corner, corner, false),
            edge: build(Component::Edge3, edge, false),
            uf: build(Component::UfCenter3, uf, true),
            slots,
            bits,
        }
    }
    fn bytes(&self) -> usize {
        size_of_val(&*self.corner) + size_of_val(&*self.edge) + size_of_val(&*self.uf)
    }
}

#[derive(Clone, Debug)]
pub(super) struct CompactIndexing {
    corner: Orbit,
    edge: Orbit,
    uf: Orbit,
    uf_bits: usize,
    uf_mask: usize,
    moves: Vec<Move>,
    packed: PackedMoves,
    // Optional exact membership in the joint pruning-coordinate orbit. Used
    // once for a search root; never queried while expanding nodes.
    joint_keys: Option<Box<[u64]>>,
}

pub(crate) struct CompactChild {
    pub corner: u16,
    pub edge3: u16,
    pub uf3: u16,
    pub edge_index: usize,
    pub corner_index: usize,
}

fn joint_key(corner: u16, edge: u16, uf: u16) -> u64 {
    u64::from(corner) | (u64::from(edge) << 16) | (u64::from(uf) << 32)
}

impl CompactIndexing {
    pub(super) fn build(tables: &TransitionTables, moves: &[Move], root: FtoCoord) -> Option<Self> {
        if (0..8).all(|axis| moves.iter().any(|mv| usize::from(mv.axis()) == axis)) {
            return None;
        }
        let corner = Orbit::build(Component::Corner, root.corner, tables, moves);
        let edge = Orbit::build(Component::Edge3, root.edge3, tables, moves);
        let uf = Orbit::build(Component::UfCenter3, root.uf_center3, tables, moves);
        // Reserve a missing UF slot and a missing row. Out-of-orbit indices
        // read 255 directly, without a membership branch in every lookup.
        let stride = (uf.original.len() + 1).next_power_of_two();
        let uf_bits = stride.trailing_zeros() as usize;
        let dense_bytes = (crate::coord::EDGE3_COUNT + CORNER_COUNT) * CENTER3_COUNT;
        let compact_values = (corner.original.len() + edge.original.len() + 2) * stride;
        let packed_width = (moves.len() + 2).next_power_of_two();
        let max_packed_bytes =
            (CORNER_COUNT + crate::coord::EDGE3_COUNT + CENTER3_COUNT) * packed_width * 8;
        if compact_values + max_packed_bytes + corner.bytes() + edge.bytes() + uf.bytes()
            >= dense_bytes / 4
        {
            return None;
        }
        let packed = PackedMoves::build(
            &corner,
            &edge,
            &uf,
            uf_bits,
            moves,
            |component, value, mv| component.next(u32::from(value), mv, tables) as u16,
        );
        let joint_keys = Self::joint_orbit(tables, moves, root);
        Some(Self {
            corner,
            edge,
            uf,
            uf_bits,
            uf_mask: stride - 1,
            moves: moves.to_vec(),
            packed,
            joint_keys,
        })
    }

    fn joint_orbit(
        tables: &TransitionTables,
        moves: &[Move],
        root: FtoCoord,
    ) -> Option<Box<[u64]>> {
        use std::collections::HashSet;
        const LIMIT: usize = 250_000;
        let mut nodes = vec![[root.corner, root.edge3, root.uf_center3]];
        let mut seen = HashSet::from([joint_key(root.corner, root.edge3, root.uf_center3)]);
        let mut head = 0;
        while head < nodes.len() {
            let [c, e, u] = nodes[head];
            head += 1;
            for &mv in moves {
                let next = [
                    tables.corner_move(c, mv),
                    tables.edge3_move(e, mv),
                    tables.uf_center3_move(u, mv),
                ];
                if seen.insert(joint_key(next[0], next[1], next[2])) {
                    if nodes.len() >= LIMIT {
                        return None;
                    }
                    nodes.push(next);
                }
            }
        }
        let mut keys = seen.into_iter().collect::<Vec<_>>();
        keys.sort_unstable();
        Some(keys.into_boxed_slice())
    }

    fn row(&self, kind: Kind) -> &Orbit {
        match kind {
            Kind::Edge => &self.edge,
            Kind::Corner => &self.corner,
        }
    }
    fn table_len(&self, kind: Kind) -> usize {
        (self.row(kind).original.len() + 1) << self.uf_bits
    }
    pub(super) fn bytes(&self) -> usize {
        self.corner.bytes()
            + self.edge.bytes()
            + self.uf.bytes()
            + self.packed.bytes()
            + self.moves.capacity() * size_of::<Move>()
            + self
                .joint_keys
                .as_ref()
                .map_or(0, |keys| size_of_val(&**keys))
    }
    fn index(&self, kind: Kind, row: u16, uf: u16) -> usize {
        let rows = self.row(kind);
        let rank = rows.rank[usize::from(row)];
        let row = if rank == OUTSIDE {
            rows.original.len()
        } else {
            usize::from(rank)
        };
        let rank = self.uf.rank[usize::from(uf)];
        let uf = if rank == OUTSIDE {
            self.uf_mask
        } else {
            usize::from(rank)
        };
        (row << self.uf_bits) | uf
    }
    pub(super) fn value(&self, kind: Kind, table: &[u8], global: usize) -> u8 {
        table[self.index(
            kind,
            (global / CENTER3_COUNT) as u16,
            (global % CENTER3_COUNT) as u16,
        )]
    }
    pub(super) fn cache_indices(
        &self,
        corner: u16,
        edge: u16,
        uf: u16,
        reject_outside: bool,
    ) -> (usize, usize) {
        if reject_outside
            && self
                .joint_keys
                .as_ref()
                .is_some_and(|keys| keys.binary_search(&joint_key(corner, edge, uf)).is_err())
        {
            return (
                self.table_len(Kind::Edge) - 1,
                self.table_len(Kind::Corner) - 1,
            );
        }
        (
            self.index(Kind::Edge, edge, uf),
            self.index(Kind::Corner, corner, uf),
        )
    }
    #[inline(always)]
    pub(super) fn step(&self, corner: u16, edge: u16, uf: u16, mv: Move) -> Option<CompactChild> {
        let slot = self.packed.slots[mv.idx()];
        if slot == u8::MAX {
            return None;
        }
        let slot = usize::from(slot);
        let c = self.packed.corner[(usize::from(corner) << self.packed.bits) | slot];
        let e = self.packed.edge[(usize::from(edge) << self.packed.bits) | slot];
        let u = self.packed.uf[(usize::from(uf) << self.packed.bits) | slot];
        Some(CompactChild {
            corner: c as u16,
            edge3: e as u16,
            uf3: u as u16,
            edge_index: (e >> 16) as usize | (u >> 16) as usize,
            corner_index: (c >> 16) as usize | (u >> 16) as usize,
        })
    }
    #[cfg(test)]
    pub(super) fn expand_for_tests(&self, kind: Kind, compact: &[u8]) -> Vec<u8> {
        let mut table = vec![UNVISITED; kind.spec().size().unwrap()];
        for (row, &original_row) in self.row(kind).original.iter().enumerate() {
            for (uf, &original_uf) in self.uf.original.iter().enumerate() {
                table[usize::from(original_row) * CENTER3_COUNT + usize::from(original_uf)] =
                    compact[(row << self.uf_bits) | uf];
            }
        }
        table
    }

    #[cfg(test)]
    pub(super) fn verify_distances_for_tests(
        &self,
        kind: Kind,
        table: &[u8],
        tables: &TransitionTables,
    ) {
        use std::collections::{HashMap, VecDeque};
        let spec = kind.spec();
        let root = usize::from(self.row(kind).original[0]) * CENTER3_COUNT
            + usize::from(self.uf.original[0]);
        let mut reference = HashMap::from([(root, 0_u8)]);
        let mut queue = VecDeque::from([root]);
        let mut values = [0; 2];
        let mut next_values = [0; 2];
        while let Some(index) = queue.pop_front() {
            let distance = reference[&index];
            for &mv in &self.moves {
                let next = spec.next_index(index, mv, tables, &mut values, &mut next_values);
                if let std::collections::hash_map::Entry::Vacant(entry) = reference.entry(next) {
                    entry.insert(distance + 1);
                    queue.push_back(next);
                }
            }
        }
        for (row, &original) in self.row(kind).original.iter().enumerate() {
            for (uf, &value) in self.uf.original.iter().enumerate() {
                let global = usize::from(original) * CENTER3_COUNT + usize::from(value);
                assert_eq!(
                    table[(row << self.uf_bits) | uf],
                    reference.get(&global).copied().unwrap_or(UNVISITED)
                );
            }
        }
        assert_eq!(
            table.iter().filter(|&&v| v != UNVISITED).count(),
            reference.len()
        );
    }
    pub(super) fn build_table(
        &self,
        kind: Kind,
        interval: usize,
        cancel: Option<&AtomicBool>,
        report: Option<&PruningReporter<'_>>,
    ) -> Result<Vec<u8>, String> {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(CANCELLED.to_owned());
        }
        let rows = self.row(kind);
        let mut table = vec![UNVISITED; self.table_len(kind)];
        table[0] = 0;
        let mut queue = std::collections::VecDeque::from([0_u32]);
        let total = rows.original.len() * self.uf.original.len();
        let mut reached = 1;
        let mut expanded = 0;
        let mut next_report = interval;
        let steps = match kind {
            Kind::Edge => &self.packed.edge,
            Kind::Corner => &self.packed.corner,
        };
        while let Some(index) = queue.pop_front() {
            if reached == total {
                break;
            }
            if expanded & 8191 == 0 && cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(CANCELLED.to_owned());
            }
            let index = index as usize;
            let depth = table[index];
            expanded += 1;
            let row = usize::from(rows.original[index >> self.uf_bits]);
            let uf = usize::from(self.uf.original[index & self.uf_mask]);
            for &mv in &self.moves {
                let slot = usize::from(self.packed.slots[mv.idx()]);
                let next = ((steps[(row << self.packed.bits) | slot] >> 16)
                    | (self.packed.uf[(uf << self.packed.bits) | slot] >> 16))
                    as usize;
                if table[next] == UNVISITED {
                    if depth >= UNVISITED - 1 {
                        return Err(
                            "pruning distance exceeds the table's byte representation".to_owned()
                        );
                    }
                    table[next] = depth + 1;
                    queue.push_back(next as u32);
                    reached += 1;
                }
            }
            if interval > 0 && expanded >= next_report {
                if let Some(report) = report {
                    report(PruningProgress {
                        name: kind.spec().name(),
                        depth: usize::from(depth),
                        expanded,
                        reached,
                        total,
                    });
                }
                next_report = expanded.saturating_add(interval);
            }
        }
        if let Some(report) = report {
            report(PruningProgress {
                name: kind.spec().name(),
                depth: table
                    .iter()
                    .copied()
                    .filter(|&v| v != UNVISITED)
                    .max()
                    .unwrap_or(0)
                    .into(),
                expanded,
                reached,
                total,
            });
        }
        Ok(table)
    }
    pub(super) fn load_or_build(
        &self,
        kind: Kind,
        path: &Path,
        interval: usize,
        cancel: Option<&AtomicBool>,
        report: Option<&PruningReporter<'_>>,
    ) -> Result<Vec<u8>, String> {
        if let Ok(table) = self.load(kind, path, report) {
            return Ok(table);
        }
        let table = self.build_table(kind, interval, cancel, report)?;
        self.save(kind, path, &table, report)
            .map_err(|e| e.to_string())?;
        Ok(table)
    }

    fn load(
        &self,
        kind: Kind,
        path: &Path,
        report: Option<&PruningReporter<'_>>,
    ) -> io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut magic = [0; 16];
        file.read_exact(&mut magic)?;
        let start = if &magic == PRUN_MAGIC {
            let mut bytes = [0; 4];
            file.read_exact(&mut bytes)?;
            20 + u64::from(u32::from_le_bytes(bytes))
        } else {
            0
        };
        let expected = kind.spec().size().unwrap() as u64;
        if start > len || len - start != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pruning table length mismatch",
            ));
        }
        let rows = self.row(kind);
        let total = rows.original.len() * CENTER3_COUNT;
        let name = format!("loading {}", kind.spec().name());
        report_table_io(report, &name, 0, total);
        let mut table = vec![UNVISITED; self.table_len(kind)];
        let mut buffer = vec![UNVISITED; CENTER3_COUNT];
        let interval = (rows.original.len() / 100).max(1);
        for (rank, &original) in rows.original.iter().enumerate() {
            file.seek(SeekFrom::Start(
                start + u64::from(original) * CENTER3_COUNT as u64,
            ))?;
            file.read_exact(&mut buffer)?;
            for (uf, &value) in self.uf.original.iter().enumerate() {
                table[(rank << self.uf_bits) | uf] = buffer[usize::from(value)];
            }
            if rank % interval == 0 || rank + 1 == rows.original.len() {
                report_table_io(report, &name, (rank + 1) * CENTER3_COUNT, total);
            }
        }
        Ok(table)
    }

    fn save(
        &self,
        kind: Kind,
        path: &Path,
        table: &[u8],
        report: Option<&PruningReporter<'_>>,
    ) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = BufWriter::new(File::create(path)?);
        let metadata = format!(
            "spec={}\nmoves={}\n",
            kind.spec().name(),
            move_set_suffix(&self.moves)
        );
        writer.write_all(PRUN_MAGIC)?;
        writer.write_all(&(metadata.len() as u32).to_le_bytes())?;
        writer.write_all(metadata.as_bytes())?;
        let rows = self.row(kind);
        let empty = vec![UNVISITED; CENTER3_COUNT];
        let mut buffer = empty.clone();
        let total = kind.spec().size().unwrap();
        let name = format!("saving {}", kind.spec().name());
        report_table_io(report, &name, 0, total);
        let interval = (rows.rank.len() / 100).max(1);
        for (original, &rank) in rows.rank.iter().enumerate() {
            if rank == OUTSIDE {
                writer.write_all(&empty)?;
            } else {
                for (uf, &value) in self.uf.original.iter().enumerate() {
                    buffer[usize::from(value)] = table[(usize::from(rank) << self.uf_bits) | uf];
                }
                writer.write_all(&buffer)?;
            }
            if original % interval == 0 || original + 1 == rows.rank.len() {
                report_table_io(report, &name, (original + 1) * CENTER3_COUNT, total);
            }
        }
        writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orbit(size: usize, values: &[u16]) -> Orbit {
        let mut rank = vec![OUTSIDE; size];
        for (id, &value) in values.iter().enumerate() {
            rank[usize::from(value)] = id as u16;
        }
        Orbit {
            original: values.to_vec().into_boxed_slice(),
            rank: rank.into_boxed_slice(),
        }
    }
    fn fixture(fixed_uf: bool) -> CompactIndexing {
        let corner = orbit(CORNER_COUNT, &[0, 7, 37]);
        let edge = orbit(crate::coord::EDGE3_COUNT, &[0, 7, 37]);
        let uf = orbit(CENTER3_COUNT, if fixed_uf { &[0] } else { &[0, 7, 37] });
        let bits = if fixed_uf { 1 } else { 2 };
        let moves = vec![Move::U, Move::Up];
        let packed =
            PackedMoves::build(&corner, &edge, &uf, bits, &moves, |component, value, mv| {
                if component == Component::UfCenter3 && fixed_uf {
                    return value;
                }
                let values = [0_u16, 7, 37];
                let Some(index) = values.iter().position(|&v| v == value) else {
                    return value;
                };
                values[(index + if mv == Move::U { 1 } else { 2 }) % 3]
            });
        let mut joint_keys =
            [0, 7, 37].map(|value| joint_key(value, value, if fixed_uf { 0 } else { value }));
        joint_keys.sort_unstable();
        CompactIndexing {
            corner,
            edge,
            uf,
            uf_bits: bits,
            uf_mask: (1 << bits) - 1,
            moves,
            packed,
            joint_keys: Some(joint_keys.to_vec().into_boxed_slice()),
        }
    }
    #[test]
    fn generation_preserves_joint_constraints_padding_and_missing_sentinels() {
        let indexing = fixture(false);
        for kind in [Kind::Edge, Kind::Corner] {
            let table = indexing.build_table(kind, 0, None, None).unwrap();
            assert_eq!(table.len(), 16);
            for row in 0..4 {
                for uf in 0..4 {
                    let expected = if row != uf || row == 3 {
                        UNVISITED
                    } else if row == 0 {
                        0
                    } else {
                        1
                    };
                    assert_eq!(table[(row << 2) | uf], expected);
                }
            }
            assert_eq!(
                indexing.value(kind, &table, 3 * CENTER3_COUNT + 3),
                UNVISITED
            );
            let mut state = (0, 0, 0);
            for expected in [7, 37, 0, 7] {
                let child = indexing.step(state.0, state.1, state.2, Move::U).unwrap();
                assert_eq!(
                    (child.corner, child.edge3, child.uf3),
                    (expected, expected, expected)
                );
                assert_eq!(
                    table[match kind {
                        Kind::Edge => child.edge_index,
                        Kind::Corner => child.corner_index,
                    }],
                    u8::from(expected != 0)
                );
                state = (child.corner, child.edge3, child.uf3);
            }
            let outside = indexing.step(3, 3, 3, Move::U).unwrap();
            assert_eq!(table[outside.edge_index], UNVISITED);
        }
        let cancel = AtomicBool::new(true);
        assert_eq!(
            indexing
                .build_table(Kind::Edge, 0, Some(&cancel), None)
                .unwrap_err(),
            CANCELLED
        );
    }
    #[test]
    fn joint_membership_rejects_cases_that_pass_the_separate_projections() {
        let indexing = fixture(true);
        let edge = indexing.build_table(Kind::Edge, 0, None, None).unwrap();
        let corner = indexing.build_table(Kind::Corner, 0, None, None).unwrap();
        let (e, c) = indexing.cache_indices(7, 37, 0, false);
        assert_eq!((edge[e], corner[c]), (1, 1));
        let (e, c) = indexing.cache_indices(7, 37, 0, true);
        assert_eq!((edge[e], corner[c]), (UNVISITED, UNVISITED));
    }
    #[test]
    fn loads_old_cache_formats_without_allocating_the_logical_table() {
        let indexing = fixture(false);
        let expected = indexing.build_table(Kind::Corner, 0, None, None).unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("fto-subgroup-{nonce}.prun"));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        for metadata in [false, true] {
            let mut file = File::create(&path).unwrap();
            let start = if metadata {
                file.write_all(PRUN_MAGIC).unwrap();
                file.write_all(&0_u32.to_le_bytes()).unwrap();
                20
            } else {
                0
            };
            file.set_len(start + Kind::Corner.spec().size().unwrap() as u64)
                .unwrap();
            for (row, &original) in indexing.corner.original.iter().enumerate() {
                let mut values = vec![UNVISITED; CENTER3_COUNT];
                for (uf, &value) in indexing.uf.original.iter().enumerate() {
                    values[usize::from(value)] = expected[(row << 2) | uf];
                }
                file.seek(SeekFrom::Start(
                    start + u64::from(original) * CENTER3_COUNT as u64,
                ))
                .unwrap();
                file.write_all(&values).unwrap();
            }
            drop(file);
            assert_eq!(indexing.load(Kind::Corner, &path, None).unwrap(), expected);
        }
        File::create(&path).unwrap().set_len(7).unwrap();
        assert!(indexing.load(Kind::Corner, &path, None).is_err());
    }
}
