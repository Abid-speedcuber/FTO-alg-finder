use crate::cubie::FtoCubie;

pub const CORNER_COUNT: usize = 11_520;
pub const CENTER_COUNT: usize = 369_600;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FtoCoord {
    pub corner: u16,
    pub edge_pack: u64,
    pub uf_center: u32,
    pub rl_center: u32,
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
            edge_pack: pack_perm12(&cubie.ep),
            uf_center: rank_center_colors(&center_colors(&cubie.uf)),
            rl_center: rank_center_colors(&center_colors(&cubie.rl)),
        }
    }

    #[must_use]
    pub fn is_solved(self) -> bool {
        self == Self::solved()
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
pub fn pack_perm12(perm: &[u8; 12]) -> u64 {
    let mut packed = 0_u64;
    for (i, &value) in perm.iter().enumerate() {
        packed |= u64::from(value) << (i * 4);
    }
    packed
}

#[must_use]
pub fn unpack_perm12(packed: u64) -> [u8; 12] {
    let mut perm = [0; 12];
    for (i, value) in perm.iter_mut().enumerate() {
        *value = ((packed >> (i * 4)) & 0x0f) as u8;
    }
    perm
}

#[must_use]
pub fn apply_packed_perm12(packed: u64, move_perm: &[u8; 12]) -> u64 {
    let mut next = 0_u64;
    for (dst, &src) in move_perm.iter().enumerate() {
        let value = (packed >> (usize::from(src) * 4)) & 0x0f;
        next |= value << (dst * 4);
    }
    next
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
pub fn unrank_center_colors(mut rank: u32) -> [u8; 12] {
    let mut remaining = [3_u8; 4];
    let mut colors = [0; 12];
    for (i, slot) in colors.iter_mut().enumerate() {
        for color in 0..4 {
            if remaining[color] == 0 {
                continue;
            }
            remaining[color] -= 1;
            let count = count_center_suffix(12 - i - 1, &remaining);
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
        rank_center_colors, rank_corner, unrank_center_colors, unrank_corner, CENTER_COUNT,
        CORNER_COUNT,
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
    #[ignore = "checks all 369600 center multiset states"]
    fn center_rank_round_trips_all_states() {
        for rank in 0..CENTER_COUNT {
            let colors = unrank_center_colors(rank as u32);
            assert_eq!(rank_center_colors(&colors), rank as u32);
        }
    }
}
