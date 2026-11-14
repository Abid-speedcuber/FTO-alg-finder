//! Input notation, including absolute {up,front} execution holds.
use crate::moves::Move;

const FACES: [&str; 8] = ["U", "F", "BR", "BL", "D", "B", "R", "L"];
const OPPOSITE: [usize; 8] = [4, 5, 7, 6, 0, 1, 3, 2];
const SIDES: [usize; 6] = [1, 7, 3, 5, 2, 6];
type Vector = [f64; 3];
fn dot(a: Vector, b: Vector) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn project(v: Vector, axis: Vector) -> Vector {
    let d = dot(v, axis);
    std::array::from_fn(|i| v[i] - d * axis[i])
}
fn input_hold(marker: &str) -> Result<[usize; 8], String> {
    let normalized: String = marker
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase();
    let parts: Vec<_> = normalized.split(',').collect();
    let invalid = || format!("invalid input orientation: {{{marker}}}");
    if parts.len() != 2 {
        return Err(invalid());
    }
    let up = FACES
        .iter()
        .position(|&f| f == parts[0])
        .ok_or_else(invalid)?;
    let front = FACES
        .iter()
        .position(|&f| f == parts[1])
        .ok_or_else(invalid)?;
    if front == up || front == OPPOSITE[up] {
        return Err(invalid());
    }
    let a = 6_f64.sqrt() / 3.;
    let b = 2_f64.sqrt() / 3.;
    let c = 8_f64.sqrt() / 3.;
    let normals = [
        [0., 1., 0.],
        [0., -1. / 3., c],
        [a, -1. / 3., -b],
        [-a, -1. / 3., -b],
        [0., -1., 0.],
        [0., 1. / 3., -c],
        [a, 1. / 3., b],
        [-a, 1. / 3., b],
    ];
    let axis = normals[up];
    let forward = project(normals[front], axis);
    let angle = |face: usize| {
        if face == front {
            return 0.;
        }
        let v = project(normals[face], axis);
        let cross = [
            forward[1] * v[2] - forward[2] * v[1],
            forward[2] * v[0] - forward[0] * v[2],
            forward[0] * v[1] - forward[1] * v[0],
        ];
        (-dot(axis, cross))
            .atan2(dot(forward, v))
            .rem_euclid(std::f64::consts::TAU)
    };
    let mut ring: Vec<_> = (0..8).filter(|&f| f != up && f != OPPOSITE[up]).collect();
    ring.sort_by(|&a, &b| angle(a).total_cmp(&angle(b)));
    let mut map = [0; 8];
    map[0] = up;
    map[4] = OPPOSITE[up];
    for (i, side) in SIDES.iter().enumerate() {
        map[*side] = ring[i];
    }
    Ok(map)
}

fn parse_token(token: &str, hold: &[usize; 8]) -> Result<Move, String> {
    let inverse = token.ends_with(['\'', '’', 'i', '2']);
    let base = if inverse {
        &token[..token.char_indices().last().unwrap().0]
    } else {
        token
    };
    let (face, kind, reverse) = match base {
        "M" => (6, 's', true),
        "E" => (0, 's', true),
        "S" => (1, 's', false),
        "N" => (7, 's', false),
        _ => {
            let (name, kind) = if let Some(face) = base.strip_suffix('w') {
                (face.to_owned(), 'w')
            } else if let Some(face) = base.strip_suffix('s') {
                (face.to_owned(), 's')
            } else if base == base.to_lowercase() {
                (base.to_uppercase(), 'w')
            } else {
                (base.to_owned(), 'f')
            };
            let face = FACES
                .iter()
                .position(|&f| f == name)
                .ok_or_else(|| format!("unknown move: {token}"))?;
            (face, kind, false)
        }
    };
    let mut face = hold[face];
    let mut inverse = inverse ^ reverse;
    if kind == 's' && [4, 5, 2, 3].contains(&face) {
        face = OPPOSITE[face];
        inverse = !inverse;
    }
    let name = match kind {
        'w' => FACES[face].to_lowercase(),
        's' => format!("{}s", FACES[face]),
        _ => FACES[face].to_owned(),
    };
    let name = format!("{name}{}", if inverse { "'" } else { "" });
    Move::ALL
        .into_iter()
        .find(|mv| mv.name() == name)
        .ok_or_else(|| format!("unknown move: {token}"))
}

pub fn parse_algorithm(algorithm: &str) -> Result<Vec<Move>, String> {
    let mut chars = algorithm.chars().peekable();
    let mut hold = std::array::from_fn(|i| i);
    let mut moves = Vec::new();
    while let Some(c) = chars.next() {
        if c.is_whitespace() || c == '(' || c == ')' {
            continue;
        }
        if c == '{' {
            let mut marker = String::new();
            let mut closed = false;
            for next in chars.by_ref() {
                if next == '}' {
                    closed = true;
                    break;
                }
                marker.push(next);
            }
            if !closed {
                return Err("unclosed input orientation".into());
            }
            hold = input_hold(&marker)?;
        } else {
            if c == '}' {
                return Err("unexpected } in algorithm".into());
            }
            let mut token = c.to_string();
            while chars
                .peek()
                .is_some_and(|c| !c.is_whitespace() && !"{}()".contains(*c))
            {
                token.push(chars.next().unwrap());
            }
            moves.push(parse_token(&token, &hold)?);
        }
    }
    Ok(moves)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_holds_and_aliases() {
        for (input, expected) in [
            ("{U,R} R U R' U'", "BR U BR' U'"),
            ("R' {U,L} R U R' U'", "R' F U F' U'"),
            ("{U,R} R {U,F} R", "BR R"),
            (
                "Uw Fw' Rw Lw' M M' E E' S S' N N'",
                "u f' r l' Rs' Rs Us' Us Fs Fs' Ls Ls'",
            ),
        ] {
            assert_eq!(
                parse_algorithm(input).unwrap(),
                parse_algorithm(expected).unwrap(),
                "{input}"
            );
        }
        for mv in Move::ALL {
            if !mv.name().starts_with('(') {
                assert_eq!(parse_algorithm(mv.name()).unwrap(), vec![mv]);
            }
        }
        for bad in ["{U,D} R", "{U,U} R", "{X,R} R", "{U,R", "R } U"] {
            assert!(parse_algorithm(bad).is_err());
        }
    }
    #[test]
    fn all_holds_map_faces_wides_and_slices_consistently() {
        for up in FACES {
            for front in FACES {
                let Ok(hold) = input_hold(&format!("{up},{front}")) else {
                    continue;
                };
                for (local, original) in hold.into_iter().enumerate() {
                    for (kind, suffix) in [("", ""), ("", "'"), ("w", ""), ("s", "'")] {
                        let input = format!("{{{up},{front}}} {}{kind}{suffix}", FACES[local]);
                        let expected = format!("{}{kind}{suffix}", FACES[original]);
                        assert_eq!(
                            parse_algorithm(&input).unwrap(),
                            parse_algorithm(&expected).unwrap()
                        );
                    }
                }
            }
        }
    }
}
