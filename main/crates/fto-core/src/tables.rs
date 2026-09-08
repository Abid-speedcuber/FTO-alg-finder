use crate::{
    coord::{
        apply_choice6_bitmap, rank_center_colors, rank_choice6, rank_corner, unrank_center_colors,
        unrank_choice6, unrank_corner, CENTER_COUNT, CORNER_COUNT, EDGE_CHOICE_COUNT,
    },
    moves::{move_cubies, Move, MOVE_COUNT},
    FtoCoord, FtoCubie,
};
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::Path,
};

const CACHE_MAGIC: &[u8; 16] = b"FTO_TRANS_V2\0\0\0\0";

#[derive(Debug)]
pub struct TransitionTables {
    corner: Vec<[u16; MOVE_COUNT]>,
    edge_choice: Vec<[u16; MOVE_COUNT]>,
    uf_center: Vec<[u32; MOVE_COUNT]>,
    rl_center: Vec<[u32; MOVE_COUNT]>,
}

impl TransitionTables {
    #[must_use]
    pub fn build() -> Self {
        let moves = move_cubies();
        Self {
            corner: build_corner_table(&moves),
            edge_choice: build_edge_choice_table(&moves),
            uf_center: build_center_table(&moves, CenterOrbit::Uf),
            rl_center: build_center_table(&moves, CenterOrbit::Rl),
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
        write_u32(&mut writer, self.uf_center.len() as u32)?;
        write_u32(&mut writer, self.rl_center.len() as u32)?;

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
        for table in [&self.uf_center, &self.rl_center] {
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
        let uf_len = read_u32(&mut reader)? as usize;
        let rl_len = read_u32(&mut reader)? as usize;
        if corner_len != CORNER_COUNT
            || edge_choice_len != EDGE_CHOICE_COUNT
            || uf_len != CENTER_COUNT
            || rl_len != CENTER_COUNT
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

        Ok(Self {
            corner,
            edge_choice,
            uf_center,
            rl_center,
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
            uf_center: self.uf_center[coord.uf_center as usize][move_idx],
            rl_center: self.rl_center[coord.rl_center as usize][move_idx],
        }
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
            let mut next = [0; 12];
            for i in 0..12 {
                next[i] = colors[perm[i] as usize];
            }
            row[move_idx] = rank_center_colors(&next);
        }
    }
    table
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
