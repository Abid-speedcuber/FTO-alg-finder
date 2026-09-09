use crate::cubie::FtoCubie;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Move {
    U = 0,
    Up = 1,
    B = 2,
    Bp = 3,
    R = 4,
    Rp = 5,
    L = 6,
    Lp = 7,
    Rw = 8,
    Rwp = 9,
}

impl Move {
    pub const ALL: [Self; 10] = [
        Self::U,
        Self::Up,
        Self::B,
        Self::Bp,
        Self::R,
        Self::Rp,
        Self::L,
        Self::Lp,
        Self::Rw,
        Self::Rwp,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::U => "U",
            Self::Up => "U'",
            Self::B => "B",
            Self::Bp => "B'",
            Self::R => "R",
            Self::Rp => "R'",
            Self::L => "L",
            Self::Lp => "L'",
            Self::Rw => "Rw",
            Self::Rwp => "Rw'",
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

pub const MOVE_COUNT: usize = 10;

#[must_use]
pub fn move_cubies() -> [FtoCubie; MOVE_COUNT] {
    let all = cstimer_move_cubies();
    [
        all[0], all[1], all[10], all[11], all[12], all[13], all[14], all[15], all[20], all[21],
    ]
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
