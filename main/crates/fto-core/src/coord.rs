use crate::cubie::FtoCubie;

pub const CORNER_COUNT: usize = 11_520;
pub const CENTER_COUNT: usize = 369_600;
pub const EDGE_CHOICE_COUNT: usize = 924;
pub const EDGE3_COUNT: usize = 34_650;
pub const EDGE4_COUNT: usize = 369_600;
pub const CENTER2_COUNT: usize = 924;
pub const CENTER3_COUNT: usize = 18_480;

pub const EDGE3_COUNTS: [u8; 3] = [4, 4, 4];
pub const EDGE4_COUNTS: [u8; 4] = [3, 3, 3, 3];
pub const CENTER2_COUNTS: [u8; 2] = [6, 6];
pub const CENTER3_COUNTS: [u8; 3] = [3, 3, 6];
pub const CENTER4_COUNTS: [u8; 4] = [3, 3, 3, 3];

const EDGE_SIGNATURES: [u8; 12] = [0, 1, 2, 3, 4, 5, 10, 11, 12, 13, 14, 15];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FtoCoord {
    pub corner: u16,
    pub edge: EdgeCoord,
    pub edge3: u16,
    pub edge4: u32,
    pub uf_center: u32,
    pub rl_center: u32,
    pub uf_center2: u16,
    pub uf_center3: u16,
    pub rl_center2: u16,
    pub rl_center3: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdgeCoord {
    pub e0: u16,
    pub e1: u16,
    pub e2: u16,
    pub e3: u16,
}

impl FtoCoord {
    #[must_use]
    pub fn solved() -> Self {
        Self::from_cubie(&FtoCubie::solved())
    }

    #[must_use]
    pub fn from_cubie(cubie: &FtoCubie) -> Self {
        Self {
            corner: rank_corner(&cubie.cp, &cubie.co),
            edge: EdgeCoord::from_ep(&cubie.ep),
            edge3: rank_equal_piece_groups(&cubie.ep, 4) as u16,
            edge4: rank_equal_piece_groups(&cubie.ep, 3),
            uf_center: rank_center_colors(&center_colors(&cubie.uf)),
            rl_center: rank_center_colors(&center_colors(&cubie.rl)),
            uf_center2: rank_center_color_groups(&cubie.uf, &[0, 0, 1, 1], &CENTER2_COUNTS) as u16,
            uf_center3: rank_center_color_groups(&cubie.uf, &[0, 1, 2, 2], &CENTER3_COUNTS) as u16,
            rl_center2: rank_center_color_groups(&cubie.rl, &[0, 0, 1, 1], &CENTER2_COUNTS) as u16,
            rl_center3: rank_center_color_groups(&cubie.rl, &[0, 1, 2, 2], &CENTER3_COUNTS) as u16,
        }
    }

    #[must_use]
    pub fn is_solved(self) -> bool {
        self == Self::solved()
    }
}

impl EdgeCoord {
    #[must_use]
    pub fn from_ep(ep: &[u8; 12]) -> Self {
        let mut bitmaps = [0_u16; 4];
        for (pos, &edge) in ep.iter().enumerate() {
            let signature = EDGE_SIGNATURES[edge as usize];
            for (bit, bitmap) in bitmaps.iter_mut().enumerate() {
                if (signature >> bit) & 1 == 1 {
                    *bitmap |= 1 << pos;
                }
            }
        }
        Self {
            e0: rank_choice6(bitmaps[0]),
            e1: rank_choice6(bitmaps[1]),
            e2: rank_choice6(bitmaps[2]),
            e3: rank_choice6(bitmaps[3]),
        }
    }

    #[must_use]
    pub fn to_ep(self) -> [u8; 12] {
        let bitmaps = [
            unrank_choice6(self.e0),
            unrank_choice6(self.e1),
            unrank_choice6(self.e2),
            unrank_choice6(self.e3),
        ];
        let mut ep = [0; 12];
        for (pos, edge) in ep.iter_mut().enumerate() {
            let mut signature = 0_u8;
            for (bit, bitmap) in bitmaps.iter().enumerate() {
                if (bitmap >> pos) & 1 == 1 {
                    signature |= 1 << bit;
                }
            }
            *edge = EDGE_SIGNATURES
                .iter()
                .position(|&candidate| candidate == signature)
                .expect("edge coloring tuple must use a valid signature") as u8;
        }
        ep
    }
}

#[must_use]
pub fn center_colors(centers: &[u8; 12]) -> [u8; 12] {
    let mut colors = [0; 12];
    for i in 0..12 {
        colors[i] = centers[i] / 3;
    }
    colors
}

#[must_use]
pub fn piece_group_colors(pieces: &[u8; 12], group_size: u8) -> [u8; 12] {
    let mut colors = [0; 12];
    for i in 0..12 {
        colors[i] = pieces[i] / group_size;
    }
    colors
}

#[must_use]
pub fn center_color_group_colors(centers: &[u8; 12], color_map: &[u8]) -> [u8; 12] {
    let mut colors = [0; 12];
    for i in 0..12 {
        colors[i] = color_map[(centers[i] / 3) as usize];
    }
    colors
}

#[must_use]
pub fn apply_color_perm(colors: &[u8; 12], move_perm: &[u8; 12]) -> [u8; 12] {
    let mut next = [0; 12];
    for i in 0..12 {
        next[i] = colors[move_perm[i] as usize];
    }
    next
}

#[must_use]
pub fn apply_choice6_bitmap(bitmap: u16, move_perm: &[u8; 12]) -> u16 {
    let mut next = 0_u16;
    for (dst, &src) in move_perm.iter().enumerate() {
        let bit = (bitmap >> src) & 1;
        next |= bit << dst;
    }
    next
}

#[must_use]
pub fn rank_choice6(bitmap: u16) -> u16 {
    assert_eq!(bitmap.count_ones(), 6);
    let mut rank = 0_u16;
    for candidate in 0_u16..=0x0fff {
        if candidate.count_ones() != 6 {
            continue;
        }
        if candidate == bitmap {
            return rank;
        }
        rank += 1;
    }
    panic!("choice bitmap is not rankable");
}

#[must_use]
pub fn unrank_choice6(mut rank: u16) -> u16 {
    for candidate in 0_u16..=0x0fff {
        if candidate.count_ones() != 6 {
            continue;
        }
        if rank == 0 {
            return candidate;
        }
        rank -= 1;
    }
    panic!("invalid choice rank");
}

#[must_use]
pub fn rank_corner(cp: &[u8; 6], co: &[u8; 6]) -> u16 {
    assert_eq!(co.iter().fold(0, |acc, &ori| acc ^ ori), 0);
    assert_eq!(perm_parity(cp), 0);

    let perm_rank = even_perm_rank6(cp);
    let mut ori_rank = 0_u16;
    for (i, &ori) in co.iter().take(5).enumerate() {
        ori_rank |= u16::from(ori) << i;
    }
    perm_rank * 32 + ori_rank
}

#[must_use]
pub fn unrank_corner(rank: u16) -> ([u8; 6], [u8; 6]) {
    let perm_rank = rank / 32;
    let ori_rank = rank % 32;
    let cp = even_perm_unrank6(perm_rank);
    let mut co = [0; 6];
    let mut parity = 0;
    for (i, ori) in co.iter_mut().take(5).enumerate() {
        *ori = ((ori_rank >> i) & 1) as u8;
        parity ^= *ori;
    }
    co[5] = parity;
    (cp, co)
}

#[must_use]
pub fn rank_center_colors(colors: &[u8; 12]) -> u32 {
    let mut remaining = [3_u8; 4];
    let mut rank = 0_u32;
    for (i, &color) in colors.iter().enumerate() {
        for smaller in 0..color {
            if remaining[smaller as usize] == 0 {
                continue;
            }
            remaining[smaller as usize] -= 1;
            rank += count_center_suffix(12 - i - 1, &remaining);
            remaining[smaller as usize] += 1;
        }
        assert!(color < 4);
        assert!(remaining[color as usize] > 0);
        remaining[color as usize] -= 1;
    }
    rank
}

#[must_use]
pub fn unrank_center_colors(rank: u32) -> [u8; 12] {
    unrank_multiset_colors(rank, &CENTER4_COUNTS)
}

#[must_use]
pub fn rank_equal_piece_groups(pieces: &[u8; 12], group_size: u8) -> u32 {
    let colors = piece_group_colors(pieces, group_size);
    let group_count = 12 / group_size;
    let counts = match group_count {
        2 => &CENTER2_COUNTS[..],
        3 => &EDGE3_COUNTS[..],
        4 => &EDGE4_COUNTS[..],
        _ => panic!("unsupported equal piece group size"),
    };
    rank_multiset_colors(&colors, counts)
}

#[must_use]
pub fn rank_center_color_groups(centers: &[u8; 12], color_map: &[u8], counts: &[u8]) -> u32 {
    rank_multiset_colors(&center_color_group_colors(centers, color_map), counts)
}

#[must_use]
pub fn rank_multiset_colors(colors: &[u8; 12], counts: &[u8]) -> u32 {
    let mut remaining = counts.to_vec();
    let mut rank = 0_u32;
    for (i, &color) in colors.iter().enumerate() {
        for smaller in 0..color {
            if remaining[smaller as usize] == 0 {
                continue;
            }
            remaining[smaller as usize] -= 1;
            rank += count_multiset_suffix(12 - i - 1, &remaining);
            remaining[smaller as usize] += 1;
        }
        assert!(usize::from(color) < remaining.len());
        assert!(remaining[color as usize] > 0);
        remaining[color as usize] -= 1;
    }
    rank
}

#[must_use]
pub fn unrank_multiset_colors(mut rank: u32, counts: &[u8]) -> [u8; 12] {
    let mut remaining = counts.to_vec();
    let mut colors = [0; 12];
    for (i, slot) in colors.iter_mut().enumerate() {
        for color in 0..remaining.len() {
            if remaining[color] == 0 {
                continue;
            }
            remaining[color] -= 1;
            let count = count_multiset_suffix(12 - i - 1, &remaining);
            if rank < count {
                *slot = color as u8;
                break;
            }
            rank -= count;
            remaining[color] += 1;
        }
    }
    colors
}

fn count_center_suffix(len: usize, remaining: &[u8; 4]) -> u32 {
    count_multiset_suffix(len, remaining)
}

fn count_multiset_suffix(len: usize, remaining: &[u8]) -> u32 {
    if remaining.iter().map(|&x| usize::from(x)).sum::<usize>() != len {
        return 0;
    }
    factorial(len)
        / remaining
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

fn even_perm_rank6(perm: &[u8; 6]) -> u16 {
    let full = lehmer_rank(perm);
    let mut rank = 0_u16;
    for candidate in 0..full {
        let p = lehmer_unrank(candidate, 6);
        if perm_parity(&p) == 0 {
            rank += 1;
        }
    }
    rank
}

fn even_perm_unrank6(rank: u16) -> [u8; 6] {
    let mut seen = 0_u16;
    for candidate in 0..720 {
        let p = lehmer_unrank(candidate, 6);
        if perm_parity(&p) == 0 {
            if seen == rank {
                let mut out = [0; 6];
                out.copy_from_slice(&p);
                return out;
            }
            seen += 1;
        }
    }
    panic!("invalid even permutation rank");
}

fn lehmer_rank(perm: &[u8]) -> u16 {
    let n = perm.len();
    let mut rank = 0_u16;
    for i in 0..n {
        let smaller = perm[i + 1..].iter().filter(|&&x| x < perm[i]).count();
        rank += u16::try_from(smaller).expect("small rank") * factorial(n - i - 1) as u16;
    }
    rank
}

fn lehmer_unrank(mut rank: u16, n: usize) -> Vec<u8> {
    let mut elems = (0..u8::try_from(n).expect("small n")).collect::<Vec<_>>();
    let mut perm = Vec::with_capacity(n);
    for i in (0..n).rev() {
        let fact = factorial(i) as u16;
        let idx = usize::from(rank / fact);
        rank %= fact;
        perm.push(elems.remove(idx));
    }
    perm
}

fn perm_parity(perm: &[u8]) -> u8 {
    let mut parity = 0;
    for i in 0..perm.len() {
        for j in i + 1..perm.len() {
            parity ^= u8::from(perm[i] > perm[j]);
        }
    }
    parity
}

#[cfg(test)]
mod tests {
    use super::{
        rank_center_colors, rank_choice6, rank_corner, rank_multiset_colors,
        unrank_center_colors, unrank_choice6, unrank_corner, unrank_multiset_colors, EdgeCoord,
        CENTER2_COUNT, CENTER2_COUNTS, CENTER3_COUNT, CENTER3_COUNTS, CENTER_COUNT, CORNER_COUNT,
        EDGE3_COUNT, EDGE3_COUNTS, EDGE4_COUNT, EDGE4_COUNTS, EDGE_CHOICE_COUNT,
    };

    #[test]
    fn corner_rank_round_trips_sample_states() {
        for rank in [0, 1, 31, 32, 1_000, CORNER_COUNT - 1] {
            let (cp, co) = unrank_corner(rank as u16);
            assert_eq!(usize::from(rank_corner(&cp, &co)), rank);
        }
    }

    #[test]
    #[ignore = "checks all 11520 legal corner states"]
    fn corner_rank_round_trips_all_legal_states() {
        for rank in 0..CORNER_COUNT {
            let (cp, co) = unrank_corner(rank as u16);
            assert_eq!(usize::from(rank_corner(&cp, &co)), rank);
        }
    }

    #[test]
    fn center_rank_round_trips_sample_states() {
        for rank in [0, 1, 42, 10_000, 123_456, CENTER_COUNT - 1] {
            let colors = unrank_center_colors(rank as u32);
            assert_eq!(rank_center_colors(&colors), rank as u32);
        }
    }

    #[test]
    fn abstract_multiset_rank_round_trips_sample_states() {
        for (count, counts) in [
            (CENTER2_COUNT, &CENTER2_COUNTS[..]),
            (CENTER3_COUNT, &CENTER3_COUNTS[..]),
            (EDGE3_COUNT, &EDGE3_COUNTS[..]),
            (EDGE4_COUNT, &EDGE4_COUNTS[..]),
        ] {
            for rank in [0, 1, count / 2, count - 1] {
                let colors = unrank_multiset_colors(rank as u32, counts);
                assert_eq!(rank_multiset_colors(&colors, counts), rank as u32);
            }
        }
    }

    #[test]
    fn edge_choice_rank_round_trips_all_states() {
        for rank in 0..EDGE_CHOICE_COUNT {
            let bitmap = unrank_choice6(rank as u16);
            assert_eq!(usize::from(rank_choice6(bitmap)), rank);
        }
    }

    #[test]
    fn edge_coloring_round_trips_solved_edges() {
        let ep = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        assert_eq!(EdgeCoord::from_ep(&ep).to_ep(), ep);
    }

    #[test]
    #[ignore = "checks all 369600 center multiset states"]
    fn center_rank_round_trips_all_states() {
        for rank in 0..CENTER_COUNT {
            let colors = unrank_center_colors(rank as u32);
            assert_eq!(rank_center_colors(&colors), rank as u32);
        }
    }
}
