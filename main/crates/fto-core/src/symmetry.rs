//! A half turn exchanging the two center orbits, with solved-piece relabeling.
use crate::{
    FtoCoord,
    coord::{
        CENTER3_COUNTS, CORNER_COUNT, rank_corner, rank_multiset_colors, unrank_center_colors,
        unrank_corner,
    },
    moves::{MOVE_COUNT, Move},
};

// Old position/identity -> new position/identity. The face map is
// (U D)(F R)(BR L)(BL B), so Fw and Rw also exchange.
const CORNERS: [usize; 6] = [5, 4, 3, 2, 1, 0];
const ORIENTATION: [u8; 6] = [0, 0, 0, 1, 1, 1];
const EDGES: [usize; 12] = [5, 4, 3, 2, 1, 0, 6, 11, 10, 9, 8, 7];
const RL_TO_UF: [usize; 12] = [2, 0, 1, 11, 9, 10, 8, 6, 7, 5, 3, 4];
#[cfg(test)]
const UF_TO_RL: [usize; 12] = [1, 2, 0, 10, 11, 9, 7, 8, 6, 4, 5, 3];

const MOVE_MAP: [Option<Move>; MOVE_COUNT] = [
    Some(Move::D),
    Some(Move::Dp),
    Some(Move::R),
    Some(Move::Rp),
    Some(Move::L),
    Some(Move::Lp),
    Some(Move::B),
    Some(Move::Bp),
    Some(Move::U),
    Some(Move::Up),
    Some(Move::BL),
    Some(Move::BLp),
    Some(Move::F),
    Some(Move::Fp),
    Some(Move::BR),
    Some(Move::BRp),
    None,
    None,
    Some(Move::Rw),
    Some(Move::Rwp),
    Some(Move::Fw),
    Some(Move::Fwp),
    None,
    None,
    Some(Move::Sp),
    Some(Move::S),
    Some(Move::Mp),
    Some(Move::M),
    Some(Move::Ep),
    Some(Move::E),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];

#[derive(Clone, Debug)]
pub(crate) struct OrbitSwap {
    corners: Box<[u16]>,
}

/// Only the extra projection is carried: rotated corners come from a small LUT.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SwappedPruning {
    pub edge3: u16,
    pub uf3: u16,
    pub bound: u8,
}

impl OrbitSwap {
    pub(crate) fn for_moves(moves: &[Move]) -> Option<Self> {
        if moves.is_empty()
            || !moves
                .iter()
                .all(|mv| Self::map_move(*mv).is_some_and(|mapped| moves.contains(&mapped)))
        {
            return None;
        }
        // Rank each permutation once; the remaining five orientation bits are
        // just permuted and XORed with its orientation gauge.
        let mut corners = vec![0; CORNER_COUNT];
        for permutation in 0..CORNER_COUNT / 32 {
            let (cp, co) = unrank_corner((permutation * 32) as u16);
            let (cp, co) = transform_corners(&cp, &co);
            let base = rank_corner(&cp, &co);
            for orientation in 0_u16..32 {
                let mut mapped = base;
                for (source, &destination) in CORNERS.iter().enumerate() {
                    if destination < 5 {
                        let bit = if source == 5 {
                            orientation.count_ones() as u16 & 1
                        } else {
                            (orientation >> source) & 1
                        };
                        mapped ^= bit << destination;
                    }
                }
                corners[permutation * 32 + usize::from(orientation)] = mapped;
            }
        }
        Some(Self {
            corners: corners.into_boxed_slice(),
        })
    }

    #[inline]
    pub(crate) fn corner(&self, index: u16) -> u16 {
        self.corners[usize::from(index)]
    }

    pub(crate) fn map_move(mv: Move) -> Option<Move> {
        MOVE_MAP[mv.idx()]
    }

    pub(crate) fn project(coord: FtoCoord) -> SwappedPruning {
        let ep = coord.edge.to_ep();
        let mut edge_colors = [0; 12];
        let rl_colors = unrank_center_colors(coord.rl_center);
        let mut center_colors = [0; 12];
        for i in 0..12 {
            edge_colors[EDGES[i]] = (EDGES[usize::from(ep[i])] / 4) as u8;
            let new_color = RL_TO_UF[usize::from(rl_colors[i]) * 3] / 3;
            center_colors[RL_TO_UF[i]] = new_color.min(2) as u8;
        }
        SwappedPruning {
            edge3: rank_multiset_colors(&edge_colors, &[4, 4, 4]) as u16,
            uf3: rank_multiset_colors(&center_colors, &CENTER3_COUNTS) as u16,
            bound: 0,
        }
    }

    pub(crate) fn bytes(&self) -> usize {
        self.corners.len() * size_of::<u16>()
    }
}

fn transform_corners(cp: &[u8; 6], co: &[u8; 6]) -> ([u8; 6], [u8; 6]) {
    let mut next_cp = [0; 6];
    let mut next_co = [0; 6];
    for i in 0..6 {
        next_cp[CORNERS[i]] = CORNERS[usize::from(cp[i])] as u8;
        next_co[CORNERS[i]] = ORIENTATION[i] ^ co[i] ^ ORIENTATION[usize::from(cp[i])];
    }
    (next_cp, next_co)
}

#[cfg(test)]
pub(crate) fn transform(cubie: crate::FtoCubie) -> crate::FtoCubie {
    let (cp, co) = transform_corners(&cubie.cp, &cubie.co);
    let mut result = crate::FtoCubie::solved();
    result.cp = cp;
    result.co = co;
    for i in 0..12 {
        result.ep[EDGES[i]] = EDGES[usize::from(cubie.ep[i])] as u8;
        result.uf[RL_TO_UF[i]] = RL_TO_UF[usize::from(cubie.rl[i])] as u8;
        result.rl[UF_TO_RL[i]] = UF_TO_RL[usize::from(cubie.uf[i])] as u8;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FtoCubie, moves::move_cubies};

    #[test]
    fn rotation_conjugates_every_supported_move_and_is_an_involution() {
        let moves = move_cubies();
        assert_eq!(transform(FtoCubie::solved()), FtoCubie::solved());
        for mv in Move::ALL {
            let rotated = transform(moves[mv.idx()]);
            assert_eq!(transform(rotated), moves[mv.idx()]);
            assert_eq!(
                Move::ALL
                    .into_iter()
                    .find(|mapped| moves[mapped.idx()] == rotated),
                OrbitSwap::map_move(mv),
                "{mv:?}"
            );
        }
    }

    #[test]
    #[ignore = "checks all rotated corner coordinates; run in release mode"]
    fn projected_rotation_matches_full_cubies_and_corner_lut_exhaustively() {
        let swap = OrbitSwap::for_moves(&Move::ALL[..16]).unwrap();
        for corner in 0..CORNER_COUNT {
            let (cp, co) = unrank_corner(corner as u16);
            let cubie = FtoCubie {
                cp,
                co,
                ..FtoCubie::solved()
            };
            assert_eq!(swap.corner(corner as u16), transform(cubie).coord().corner);
            assert_eq!(swap.corner(swap.corner(corner as u16)), corner as u16);
        }
        let mut cubie = FtoCubie::solved();
        let mut random = 7_u64;
        for _ in 0..1000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            cubie = cubie.apply(Move::ALL[(random >> 32) as usize % MOVE_COUNT]);
            let projected = OrbitSwap::project(cubie.coord());
            let rotated = transform(cubie).coord();
            assert_eq!(swap.corner(cubie.coord().corner), rotated.corner);
            assert_eq!(projected.edge3, rotated.edge3);
            assert_eq!(projected.uf3, rotated.uf_center3);
        }
    }

    #[test]
    fn the_existing_edge3_index_cannot_determine_the_rotated_index() {
        let solved = FtoCubie::solved();
        let mut same_coloring = solved;
        same_coloring.ep[..3].copy_from_slice(&[1, 2, 0]);
        assert_eq!(solved.coord().edge3, same_coloring.coord().edge3);
        assert_ne!(
            OrbitSwap::project(solved.coord()).edge3,
            OrbitSwap::project(same_coloring.coord()).edge3,
        );
    }

    #[test]
    fn only_closed_move_sets_can_reuse_the_table() {
        let mut standard = Move::ALL[..16].to_vec();
        standard.extend([Move::Fw, Move::Fwp, Move::Rw, Move::Rwp]);
        assert!(OrbitSwap::for_moves(&standard).is_some());
        assert!(OrbitSwap::for_moves(&[Move::F, Move::Fp, Move::R, Move::Rp]).is_some());
        assert!(OrbitSwap::for_moves(&[Move::R, Move::Rp, Move::U, Move::Up]).is_none());
        assert!(OrbitSwap::for_moves(&Move::ALL).is_none());
    }
}
