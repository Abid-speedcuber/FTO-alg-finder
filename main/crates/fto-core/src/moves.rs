use crate::cubie::FtoCubie;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Move {
    U = 0,
    Up = 1,
    F = 2,
    Fp = 3,
    BR = 4,
    BRp = 5,
    BL = 6,
    BLp = 7,
    D = 8,
    Dp = 9,
    B = 10,
    Bp = 11,
    R = 12,
    Rp = 13,
    L = 14,
    Lp = 15,
    Uw = 16,
    Uwp = 17,
    Fw = 18,
    Fwp = 19,
    Rw = 20,
    Rwp = 21,
    Lw = 22,
    Lwp = 23,
    M = 24,
    Mp = 25,
    S = 26,
    Sp = 27,
    E = 28,
    Ep = 29,
    RURp = 30,
    RUpRp = 31,
    RpUR = 32,
    RpUpR = 33,
    FUFp = 34,
    FUpFp = 35,
    FpUF = 36,
    FpUpF = 37,
}

impl Move {
    pub const ALL: [Self; MOVE_COUNT] = [
        Self::U,
        Self::Up,
        Self::F,
        Self::Fp,
        Self::BR,
        Self::BRp,
        Self::BL,
        Self::BLp,
        Self::D,
        Self::Dp,
        Self::B,
        Self::Bp,
        Self::R,
        Self::Rp,
        Self::L,
        Self::Lp,
        Self::Uw,
        Self::Uwp,
        Self::Fw,
        Self::Fwp,
        Self::Rw,
        Self::Rwp,
        Self::Lw,
        Self::Lwp,
        Self::M,
        Self::Mp,
        Self::S,
        Self::Sp,
        Self::E,
        Self::Ep,
        Self::RURp,
        Self::RUpRp,
        Self::RpUR,
        Self::RpUpR,
        Self::FUFp,
        Self::FUpFp,
        Self::FpUF,
        Self::FpUpF,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::U => "U",
            Self::Up => "U'",
            Self::F => "F",
            Self::Fp => "F'",
            Self::BR => "BR",
            Self::BRp => "BR'",
            Self::BL => "BL",
            Self::BLp => "BL'",
            Self::D => "D",
            Self::Dp => "D'",
            Self::B => "B",
            Self::Bp => "B'",
            Self::R => "R",
            Self::Rp => "R'",
            Self::L => "L",
            Self::Lp => "L'",
            Self::Uw => "Uw",
            Self::Uwp => "Uw'",
            Self::Fw => "Fw",
            Self::Fwp => "Fw'",
            Self::Rw => "Rw",
            Self::Rwp => "Rw'",
            Self::Lw => "Lw",
            Self::Lwp => "Lw'",
            Self::M => "M",
            Self::Mp => "M'",
            Self::S => "S",
            Self::Sp => "S'",
            Self::E => "E",
            Self::Ep => "E'",
            Self::RURp => "(R U R')",
            Self::RUpRp => "(R U' R')",
            Self::RpUR => "(R' U R)",
            Self::RpUpR => "(R' U' R)",
            Self::FUFp => "(F U F')",
            Self::FUpFp => "(F U' F')",
            Self::FpUF => "(F' U F)",
            Self::FpUpF => "(F' U' F)",
        }
    }

    #[must_use]
    pub const fn axis(self) -> u8 {
        self as u8 / 2
    }

    #[must_use]
    pub const fn idx(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn inverse(self) -> Self {
        Self::ALL[self.idx() ^ 1]
    }

    #[must_use]
    pub const fn from_idx(idx: usize) -> Self {
        Self::ALL[idx]
    }
}

pub const MOVE_COUNT: usize = 38;

#[must_use]
pub fn move_cubies() -> [FtoCubie; MOVE_COUNT] {
    let all = cstimer_move_cubies();
    let mut moves = [FtoCubie::solved(); MOVE_COUNT];
    moves[..24].copy_from_slice(&all);
    moves[Move::M.idx()] = all[Move::Rwp.idx()].compose(&all[Move::R.idx()]);
    moves[Move::Mp.idx()] = all[Move::Rw.idx()].compose(&all[Move::Rp.idx()]);
    moves[Move::S.idx()] = all[Move::Fw.idx()].compose(&all[Move::Fp.idx()]);
    moves[Move::Sp.idx()] = all[Move::Fwp.idx()].compose(&all[Move::F.idx()]);
    moves[Move::E.idx()] = all[Move::Uwp.idx()].compose(&all[Move::U.idx()]);
    moves[Move::Ep.idx()] = all[Move::Uw.idx()].compose(&all[Move::Up.idx()]);

    moves[Move::RURp.idx()] = FtoCubie::new(
        [2, 1, 5, 3, 4, 0],
        [1, 0, 1, 0, 0, 0],
        [0, 6, 1, 3, 4, 5, 2, 7, 8, 9, 10, 11],
        [0, 1, 5, 2, 4, 3, 6, 7, 8, 9, 10, 11],
        [0, 1, 2, 3, 6, 7, 9, 10, 8, 4, 5, 11],
    );
    moves[Move::RUpRp.idx()] = FtoCubie::new(
        [5, 1, 0, 3, 4, 2],
        [0, 0, 1, 0, 0, 1],
        [0, 2, 6, 3, 4, 5, 1, 7, 8, 9, 10, 11],
        [0, 1, 3, 5, 4, 2, 6, 7, 8, 9, 10, 11],
        [0, 1, 2, 3, 9, 10, 4, 5, 8, 6, 7, 11],
    );
    moves[Move::RpUR.idx()] = FtoCubie::new(
        [0, 5, 1, 3, 4, 2],
        [0, 1, 1, 0, 0, 0],
        [0, 11, 1, 3, 4, 5, 6, 7, 8, 9, 10, 2],
        [0, 1, 7, 3, 4, 5, 6, 8, 2, 9, 10, 11],
        [0, 1, 2, 3, 6, 7, 10, 11, 8, 9, 4, 5],
    );
    moves[Move::RpUpR.idx()] = FtoCubie::new(
        [0, 2, 5, 3, 4, 1],
        [0, 1, 0, 0, 0, 1],
        [0, 2, 11, 3, 4, 5, 6, 7, 8, 9, 10, 1],
        [0, 1, 8, 3, 4, 5, 6, 2, 7, 9, 10, 11],
        [0, 1, 2, 3, 10, 11, 4, 5, 8, 9, 6, 7],
    );

    moves[Move::FUFp.idx()] = all[Move::F.idx()].compose(&all[Move::U.idx()]).compose(&all[Move::Fp.idx()]);
    moves[Move::FUpFp.idx()] = all[Move::F.idx()].compose(&all[Move::Up.idx()]).compose(&all[Move::Fp.idx()]);
    moves[Move::FpUF.idx()] = all[Move::Fp.idx()].compose(&all[Move::U.idx()]).compose(&all[Move::F.idx()]);
    moves[Move::FpUpF.idx()] = all[Move::Fp.idx()].compose(&all[Move::Up.idx()]).compose(&all[Move::F.idx()]);
    moves
}

fn cstimer_move_cubies() -> [FtoCubie; 24] {
    let rot_u = FtoCubie::new(
        [1, 2, 0, 4, 5, 3],
        [0, 0, 0, 0, 0, 0],
        [2, 0, 1, 5, 3, 4, 10, 11, 6, 7, 8, 9],
        [1, 2, 0, 7, 8, 6, 10, 11, 9, 4, 5, 3],
        [2, 0, 1, 8, 6, 7, 11, 9, 10, 5, 3, 4],
    );
    let rot_r = FtoCubie::new(
        [5, 0, 4, 2, 3, 1],
        [1, 1, 0, 1, 1, 0],
        [6, 5, 7, 9, 2, 10, 11, 4, 3, 8, 1, 0],
        [5, 3, 4, 8, 6, 7, 2, 0, 1, 11, 9, 10],
        [4, 5, 3, 7, 8, 6, 1, 2, 0, 10, 11, 9],
    );
    let rot_ui = rot_u.compose(&rot_u);
    let rot_ri = rot_r.compose(&rot_r);
    let rot_l = rot_ui.compose(&rot_r).compose(&rot_u);
    let rot_f = rot_r.compose(&rot_u).compose(&rot_ri);

    let mut moves = [FtoCubie::solved(); 24];
    moves[0] = FtoCubie::new(
        [1, 2, 0, 3, 4, 5],
        [0, 0, 0, 0, 0, 0],
        [2, 0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        [1, 2, 0, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        [0, 1, 2, 3, 6, 7, 11, 9, 8, 5, 10, 4],
    );
    moves[2] = FtoCubie::new(
        [4, 1, 2, 3, 5, 0],
        [1, 0, 0, 0, 1, 0],
        [0, 1, 2, 3, 4, 6, 7, 5, 8, 9, 10, 11],
        [0, 1, 2, 4, 5, 3, 6, 7, 8, 9, 10, 11],
        [0, 9, 10, 3, 4, 5, 2, 7, 1, 8, 6, 11],
    );
    moves[4] = FtoCubie::new(
        [0, 5, 2, 1, 4, 3],
        [0, 1, 0, 0, 0, 1],
        [0, 1, 2, 3, 10, 5, 6, 7, 8, 9, 11, 4],
        [0, 1, 2, 3, 4, 5, 7, 8, 6, 9, 10, 11],
        [5, 3, 2, 11, 4, 10, 6, 7, 8, 9, 0, 1],
    );
    moves[6] = FtoCubie::new(
        [0, 1, 3, 4, 2, 5],
        [0, 0, 1, 1, 0, 0],
        [0, 1, 2, 8, 4, 5, 6, 7, 9, 3, 10, 11],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 9],
        [8, 1, 7, 2, 0, 5, 6, 3, 4, 9, 10, 11],
    );
    moves[8] = FtoCubie::new(
        [0, 1, 2, 5, 3, 4],
        [0, 0, 0, 0, 0, 0],
        [0, 1, 2, 4, 5, 3, 6, 7, 8, 9, 10, 11],
        [0, 1, 2, 3, 9, 10, 5, 7, 4, 8, 6, 11],
        [1, 2, 0, 3, 4, 5, 6, 7, 8, 9, 10, 11],
    );
    moves[10] = FtoCubie::new(
        [0, 3, 1, 2, 4, 5],
        [0, 1, 1, 0, 0, 0],
        [0, 1, 10, 3, 4, 5, 6, 7, 8, 2, 9, 11],
        [0, 6, 7, 3, 4, 5, 11, 9, 8, 2, 10, 1],
        [0, 1, 2, 4, 5, 3, 6, 7, 8, 9, 10, 11],
    );
    moves[12] = FtoCubie::new(
        [5, 0, 2, 3, 4, 1],
        [1, 1, 0, 0, 0, 0],
        [6, 1, 2, 3, 4, 5, 11, 7, 8, 9, 10, 0],
        [5, 3, 2, 8, 4, 7, 6, 0, 1, 9, 10, 11],
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 9],
    );
    moves[14] = FtoCubie::new(
        [2, 1, 4, 3, 0, 5],
        [1, 0, 1, 0, 0, 0],
        [0, 8, 2, 3, 4, 5, 6, 1, 7, 9, 10, 11],
        [11, 1, 10, 2, 0, 5, 6, 7, 8, 9, 3, 4],
        [0, 1, 2, 3, 4, 5, 7, 8, 6, 9, 10, 11],
    );
    moves[16] = rot_u.compose(&moves[8]);
    moves[18] = rot_f.compose(&moves[10]);
    moves[20] = rot_r.compose(&moves[6]);
    moves[22] = rot_l.compose(&moves[4]);

    for i in (1..24).step_by(2) {
        moves[i] = moves[i - 1].compose(&moves[i - 1]);
    }
    moves
}

#[cfg(test)]
mod tests {
    use super::Move;
    use crate::FtoCubie;

    #[test]
    fn slice_moves_match_expanded_sequences() {
        assert_eq!(
            FtoCubie::solved().apply(Move::M),
            FtoCubie::solved().apply(Move::Rwp).apply(Move::R)
        );
        assert_eq!(
            FtoCubie::solved().apply(Move::Mp),
            FtoCubie::solved().apply(Move::Rw).apply(Move::Rp)
        );
        assert_eq!(
            FtoCubie::solved().apply(Move::S),
            FtoCubie::solved().apply(Move::Fw).apply(Move::Fp)
        );
        assert_eq!(
            FtoCubie::solved().apply(Move::Sp),
            FtoCubie::solved().apply(Move::Fwp).apply(Move::F)
        );
        assert_eq!(
            FtoCubie::solved().apply(Move::E),
            FtoCubie::solved().apply(Move::Uwp).apply(Move::U)
        );
        assert_eq!(
            FtoCubie::solved().apply(Move::Ep),
            FtoCubie::solved().apply(Move::Uw).apply(Move::Up)
        );
    }
}
