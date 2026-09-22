use crate::{coord::FtoCoord, moves::Move};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FtoCubie {
    pub cp: [u8; 6],
    pub co: [u8; 6],
    pub ep: [u8; 12],
    pub uf: [u8; 12],
    pub rl: [u8; 12],
}

impl FtoCubie {
    #[must_use]
    pub const fn new(
        cp: [u8; 6],
        co: [u8; 6],
        ep: [u8; 12],
        uf: [u8; 12],
        rl: [u8; 12],
    ) -> Self {
        Self { cp, co, ep, uf, rl }
    }

    #[must_use]
    pub const fn solved() -> Self {
        Self {
            cp: [0, 1, 2, 3, 4, 5],
            co: [0, 0, 0, 0, 0, 0],
            ep: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            uf: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            rl: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    #[must_use]
    pub fn compose(&self, rhs: &Self) -> Self {
        let mut prod = Self::solved();
        for i in 0..6 {
            let src = rhs.cp[i] as usize;
            prod.co[i] = self.co[src] ^ rhs.co[i];
            prod.cp[i] = self.cp[src];
        }
        for i in 0..12 {
            prod.ep[i] = self.ep[rhs.ep[i] as usize];
            prod.uf[i] = self.uf[rhs.uf[i] as usize];
            prod.rl[i] = self.rl[rhs.rl[i] as usize];
        }
        prod
    }

    #[must_use]
    pub fn apply(self, mv: Move) -> Self {
        self.compose(&crate::moves::move_cubies()[mv.idx()])
    }

    #[must_use]
    pub fn coord(&self) -> FtoCoord {
        FtoCoord::from_cubie(self)
    }
}

impl Default for FtoCubie {
    fn default() -> Self {
        Self::solved()
    }
}
