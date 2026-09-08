use crate::{
    coord::{
        apply_choice6_bitmap, apply_color_perm, rank_center_colors, rank_choice6, rank_corner,
        rank_multiset_colors, unrank_center_colors, unrank_choice6, unrank_corner,
        unrank_multiset_colors, CENTER2_COUNT, CENTER2_COUNTS, CENTER3_COUNT, CENTER3_COUNTS,
        CENTER_COUNT, CORNER_COUNT, EDGE3_COUNT, EDGE3_COUNTS, EDGE4_COUNT, EDGE4_COUNTS,
        EDGE_CHOICE_COUNT,
    },
    moves::{move_cubies, Move, MOVE_COUNT},
    FtoCoord, FtoCubie,
};
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
};

const CACHE_MAGIC: &[u8; 16] = b"FTO_TRANS_V3\0\0\0\0";

#[derive(Debug)]
pub struct TransitionTables {
    corner: Vec<[u16; MOVE_COUNT]>,
    edge_choice: Vec<[u16; MOVE_COUNT]>,
    edge3: Vec<[u16; MOVE_COUNT]>,
    edge4: Vec<[u32; MOVE_COUNT]>,
    uf_center: Vec<[u32; MOVE_COUNT]>,
    rl_center: Vec<[u32; MOVE_COUNT]>,
    uf_center2: Vec<[u16; MOVE_COUNT]>,
    uf_center3: Vec<[u16; MOVE_COUNT]>,
    rl_center2: Vec<[u16; MOVE_COUNT]>,
    rl_center3: Vec<[u16; MOVE_COUNT]>,
}

impl TransitionTables {
    #[must_use]
    pub fn build() -> Self {
        let moves = move_cubies();
        Self {
            corner: build_corner_table(&moves),
            edge_choice: build_edge_choice_table(&moves),
            edge3: build_color_table_u16(&moves, EdgeOrCenterOrbit::Edge, &EDGE3_COUNTS),
            edge4: build_color_table_u32(&moves, EdgeOrCenterOrbit::Edge, &EDGE4_COUNTS),
            uf_center: build_center_table(&moves, CenterOrbit::Uf),
            rl_center: build_center_table(&moves, CenterOrbit::Rl),
            uf_center2: build_color_table_u16(&moves, EdgeOrCenterOrbit::UfCenter, &CENTER2_COUNTS),
            uf_center3: build_color_table_u16(&moves, EdgeOrCenterOrbit::UfCenter, &CENTER3_COUNTS),
            rl_center2: build_color_table_u16(&moves, EdgeOrCenterOrbit::RlCenter, &CENTER2_COUNTS),
            rl_center3: build_color_table_u16(&moves, EdgeOrCenterOrbit::RlCenter, &CENTER3_COUNTS),
        }
    }

    pub fn load_or_build(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        match Self::load(path) {
            Ok(tables) => Ok(tables),
            Err(_) => {
                let tables = Self::build();
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                tables.save(path)?;
                Ok(tables)
            }
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(CACHE_MAGIC)?;
        write_u32(&mut writer, self.corner.len() as u32)?;
        write_u32(&mut writer, self.edge_choice.len() as u32)?;
        write_u32(&mut writer, self.edge3.len() as u32)?;
        write_u32(&mut writer, self.edge4.len() as u32)?;
        write_u32(&mut writer, self.uf_center.len() as u32)?;
        write_u32(&mut writer, self.rl_center.len() as u32)?;
        write_u32(&mut writer, self.uf_center2.len() as u32)?;
        write_u32(&mut writer, self.uf_center3.len() as u32)?;
        write_u32(&mut writer, self.rl_center2.len() as u32)?;
        write_u32(&mut writer, self.rl_center3.len() as u32)?;

        for row in &self.corner {
            for &value in row {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        for row in &self.edge_choice {
            for &value in row {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        for row in &self.edge3 {
            for &value in row {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        for row in &self.edge4 {
            for &value in row {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        for table in [&self.uf_center, &self.rl_center] {
            for row in table {
                for &value in row {
                    writer.write_all(&value.to_le_bytes())?;
                }
            }
        }
        for table in [
            &self.uf_center2,
            &self.uf_center3,
            &self.rl_center2,
            &self.rl_center3,
        ] {
            for row in table {
                for &value in row {
                    writer.write_all(&value.to_le_bytes())?;
                }
            }
        }
        writer.flush()
    }

    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut reader = BufReader::new(File::open(path)?);
        let mut magic = [0; 16];
        reader.read_exact(&mut magic)?;
        if &magic != CACHE_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "transition cache magic mismatch",
            ));
        }
        let corner_len = read_u32(&mut reader)? as usize;
        let edge_choice_len = read_u32(&mut reader)? as usize;
        let edge3_len = read_u32(&mut reader)? as usize;
        let edge4_len = read_u32(&mut reader)? as usize;
        let uf_len = read_u32(&mut reader)? as usize;
        let rl_len = read_u32(&mut reader)? as usize;
        let uf2_len = read_u32(&mut reader)? as usize;
        let uf3_len = read_u32(&mut reader)? as usize;
        let rl2_len = read_u32(&mut reader)? as usize;
        let rl3_len = read_u32(&mut reader)? as usize;
        if corner_len != CORNER_COUNT
            || edge_choice_len != EDGE_CHOICE_COUNT
            || edge3_len != EDGE3_COUNT
            || edge4_len != EDGE4_COUNT
            || uf_len != CENTER_COUNT
            || rl_len != CENTER_COUNT
            || uf2_len != CENTER2_COUNT
            || uf3_len != CENTER3_COUNT
            || rl2_len != CENTER2_COUNT
            || rl3_len != CENTER3_COUNT
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "transition cache dimensions mismatch",
            ));
        }

        let mut corner = vec![[0; MOVE_COUNT]; CORNER_COUNT];
        for row in &mut corner {
            for value in row {
                let mut bytes = [0; 2];
                reader.read_exact(&mut bytes)?;
                *value = u16::from_le_bytes(bytes);
            }
        }

        let mut edge_choice = vec![[0; MOVE_COUNT]; EDGE_CHOICE_COUNT];
        for row in &mut edge_choice {
            for value in row {
                let mut bytes = [0; 2];
                reader.read_exact(&mut bytes)?;
                *value = u16::from_le_bytes(bytes);
            }
        }

        let mut edge3 = vec![[0; MOVE_COUNT]; EDGE3_COUNT];
        for row in &mut edge3 {
            for value in row {
                let mut bytes = [0; 2];
                reader.read_exact(&mut bytes)?;
                *value = u16::from_le_bytes(bytes);
            }
        }

        let mut edge4 = vec![[0; MOVE_COUNT]; EDGE4_COUNT];
        for row in &mut edge4 {
            for value in row {
                let mut bytes = [0; 4];
                reader.read_exact(&mut bytes)?;
                *value = u32::from_le_bytes(bytes);
            }
        }

        let mut uf_center = vec![[0; MOVE_COUNT]; CENTER_COUNT];
        let mut rl_center = vec![[0; MOVE_COUNT]; CENTER_COUNT];
        for table in [&mut uf_center, &mut rl_center] {
            for row in table {
                for value in row {
                    let mut bytes = [0; 4];
                    reader.read_exact(&mut bytes)?;
                    *value = u32::from_le_bytes(bytes);
                }
            }
        }

        let mut uf_center2 = vec![[0; MOVE_COUNT]; CENTER2_COUNT];
        let mut uf_center3 = vec![[0; MOVE_COUNT]; CENTER3_COUNT];
        let mut rl_center2 = vec![[0; MOVE_COUNT]; CENTER2_COUNT];
        let mut rl_center3 = vec![[0; MOVE_COUNT]; CENTER3_COUNT];
        for table in [
            &mut uf_center2,
            &mut uf_center3,
            &mut rl_center2,
            &mut rl_center3,
        ] {
            for row in table {
                for value in row {
                    let mut bytes = [0; 2];
                    reader.read_exact(&mut bytes)?;
                    *value = u16::from_le_bytes(bytes);
                }
            }
        }

        Ok(Self {
            corner,
            edge_choice,
            edge3,
            edge4,
            uf_center,
            rl_center,
            uf_center2,
            uf_center3,
            rl_center2,
            rl_center3,
        })
    }

    #[must_use]
    pub fn apply(&self, coord: FtoCoord, mv: Move) -> FtoCoord {
        let move_idx = mv.idx();
        let edge = coord.edge;
        FtoCoord {
            corner: self.corner[usize::from(coord.corner)][move_idx],
            edge: crate::coord::EdgeCoord {
                e0: self.edge_choice[usize::from(edge.e0)][move_idx],
                e1: self.edge_choice[usize::from(edge.e1)][move_idx],
                e2: self.edge_choice[usize::from(edge.e2)][move_idx],
                e3: self.edge_choice[usize::from(edge.e3)][move_idx],
            },
            edge3: self.edge3[usize::from(coord.edge3)][move_idx],
            edge4: self.edge4[coord.edge4 as usize][move_idx],
            uf_center: self.uf_center[coord.uf_center as usize][move_idx],
            rl_center: self.rl_center[coord.rl_center as usize][move_idx],
            uf_center2: self.uf_center2[usize::from(coord.uf_center2)][move_idx],
            uf_center3: self.uf_center3[usize::from(coord.uf_center3)][move_idx],
            rl_center2: self.rl_center2[usize::from(coord.rl_center2)][move_idx],
            rl_center3: self.rl_center3[usize::from(coord.rl_center3)][move_idx],
        }
    }

    #[must_use]
    pub fn corner_move(&self, idx: u16, mv: Move) -> u16 {
        self.corner[usize::from(idx)][mv.idx()]
    }

    #[must_use]
    pub fn edge_choice_move(&self, idx: u16, mv: Move) -> u16 {
        self.edge_choice[usize::from(idx)][mv.idx()]
    }

    #[must_use]
    pub fn edge3_move(&self, idx: u16, mv: Move) -> u16 {
        self.edge3[usize::from(idx)][mv.idx()]
    }

    #[must_use]
    pub fn edge4_move(&self, idx: u32, mv: Move) -> u32 {
        self.edge4[idx as usize][mv.idx()]
    }

    #[must_use]
    pub fn uf_center_move(&self, idx: u32, mv: Move) -> u32 {
        self.uf_center[idx as usize][mv.idx()]
    }

    #[must_use]
    pub fn rl_center_move(&self, idx: u32, mv: Move) -> u32 {
        self.rl_center[idx as usize][mv.idx()]
    }

    #[must_use]
    pub fn uf_center2_move(&self, idx: u16, mv: Move) -> u16 {
        self.uf_center2[usize::from(idx)][mv.idx()]
    }

    #[must_use]
    pub fn uf_center3_move(&self, idx: u16, mv: Move) -> u16 {
        self.uf_center3[usize::from(idx)][mv.idx()]
    }

    #[must_use]
    pub fn rl_center2_move(&self, idx: u16, mv: Move) -> u16 {
        self.rl_center2[usize::from(idx)][mv.idx()]
    }

    #[must_use]
    pub fn rl_center3_move(&self, idx: u16, mv: Move) -> u16 {
        self.rl_center3[usize::from(idx)][mv.idx()]
    }
}

fn write_u32(mut writer: impl Write, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn read_u32(mut reader: impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

enum CenterOrbit {
    Uf,
    Rl,
}

#[derive(Clone, Copy)]
enum EdgeOrCenterOrbit {
    Edge,
    UfCenter,
    RlCenter,
}

fn build_corner_table(moves: &[FtoCubie; MOVE_COUNT]) -> Vec<[u16; MOVE_COUNT]> {
    let mut table = vec![[0; MOVE_COUNT]; CORNER_COUNT];
    for (rank, row) in table.iter_mut().enumerate() {
        let (cp, co) = unrank_corner(rank as u16);
        let state = FtoCubie::new(
            cp,
            co,
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        );
        for (move_idx, mv) in moves.iter().enumerate() {
            let next = state.compose(mv);
            row[move_idx] = rank_corner(&next.cp, &next.co);
        }
    }
    table
}

fn build_edge_choice_table(moves: &[FtoCubie; MOVE_COUNT]) -> Vec<[u16; MOVE_COUNT]> {
    let mut table = vec![[0; MOVE_COUNT]; EDGE_CHOICE_COUNT];
    for (rank, row) in table.iter_mut().enumerate() {
        let bitmap = unrank_choice6(rank as u16);
        for (move_idx, mv) in moves.iter().enumerate() {
            row[move_idx] = rank_choice6(apply_choice6_bitmap(bitmap, &mv.ep));
        }
    }
    table
}

fn build_center_table(moves: &[FtoCubie; MOVE_COUNT], orbit: CenterOrbit) -> Vec<[u32; MOVE_COUNT]> {
    let mut table = vec![[0; MOVE_COUNT]; CENTER_COUNT];
    for (rank, row) in table.iter_mut().enumerate() {
        let colors = unrank_center_colors(rank as u32);
        for (move_idx, mv) in moves.iter().enumerate() {
            let perm = match orbit {
                CenterOrbit::Uf => mv.uf,
                CenterOrbit::Rl => mv.rl,
            };
            let next = apply_color_perm(&colors, &perm);
            row[move_idx] = rank_center_colors(&next);
        }
    }
    table
}

fn build_color_table_u16(
    moves: &[FtoCubie; MOVE_COUNT],
    orbit: EdgeOrCenterOrbit,
    counts: &[u8],
) -> Vec<[u16; MOVE_COUNT]> {
    let size = multiset_count(counts) as usize;
    let mut table = vec![[0; MOVE_COUNT]; size];
    for (rank, row) in table.iter_mut().enumerate() {
        let colors = unrank_multiset_colors(rank as u32, counts);
        for (move_idx, mv) in moves.iter().enumerate() {
            let next = apply_color_perm(&colors, move_perm(mv, orbit));
            row[move_idx] = rank_multiset_colors(&next, counts) as u16;
        }
    }
    table
}

fn build_color_table_u32(
    moves: &[FtoCubie; MOVE_COUNT],
    orbit: EdgeOrCenterOrbit,
    counts: &[u8],
) -> Vec<[u32; MOVE_COUNT]> {
    let size = multiset_count(counts) as usize;
    let mut table = vec![[0; MOVE_COUNT]; size];
    for (rank, row) in table.iter_mut().enumerate() {
        let colors = unrank_multiset_colors(rank as u32, counts);
        for (move_idx, mv) in moves.iter().enumerate() {
            let next = apply_color_perm(&colors, move_perm(mv, orbit));
            row[move_idx] = rank_multiset_colors(&next, counts);
        }
    }
    table
}

fn move_perm(mv: &FtoCubie, orbit: EdgeOrCenterOrbit) -> &[u8; 12] {
    match orbit {
        EdgeOrCenterOrbit::Edge => &mv.ep,
        EdgeOrCenterOrbit::UfCenter => &mv.uf,
        EdgeOrCenterOrbit::RlCenter => &mv.rl,
    }
}

fn multiset_count(counts: &[u8]) -> u32 {
    let total = counts.iter().map(|&count| usize::from(count)).sum::<usize>();
    factorial(total)
        / counts
            .iter()
            .map(|&count| factorial(usize::from(count)))
            .product::<u32>()
}

const fn factorial(n: usize) -> u32 {
    const FACT: [u32; 13] = [
        1, 1, 2, 6, 24, 120, 720, 5_040, 40_320, 362_880, 3_628_800, 39_916_800, 479_001_600,
    ];
    FACT[n]
}

#[cfg(test)]
mod tests {
    use crate::{moves::Move, tables::TransitionTables, FtoCubie};

    #[test]
    #[ignore = "builds full center transition tables"]
    fn transition_tables_match_cubie_moves_for_basic_sequence() {
        let tables = TransitionTables::build();
        let moves = [Move::R, Move::U, Move::Rp, Move::B, Move::Rw, Move::Rwp];
        let mut cubie = FtoCubie::solved();
        let mut coord = cubie.coord();

        for mv in moves {
            cubie = cubie.apply(mv);
            coord = tables.apply(coord, mv);
            assert_eq!(coord, cubie.coord());
        }
    }
}
