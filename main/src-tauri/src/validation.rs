use super::*;

#[derive(Debug, Serialize)]
pub(super) struct ValidationIssue {
    message: String,
    color: Option<u8>,
}

#[derive(Serialize)]
pub(super) struct ValidationReport {
    state: Option<CubieState>,
    errors: Vec<ValidationIssue>,
}

fn issue(errors: &mut Vec<ValidationIssue>, message: impl Into<String>) {
    errors.push(ValidationIssue {
        message: message.into(),
        color: None,
    });
}

pub(super) fn validate(facelets: &[u8]) -> ValidationReport {
    let mut errors = Vec::new();
    if facelets.len() != 72 {
        issue(
            &mut errors,
            format!("The puzzle has {} stickers; expected 72.", facelets.len()),
        );
        return ValidationReport {
            state: None,
            errors,
        };
    }
    for (slot, &color) in facelets.iter().enumerate() {
        if color > 8 {
            issue(
                &mut errors,
                format!(
                    "Sticker {} has an unknown color. Choose a color from the palette.",
                    slot + 1
                ),
            );
        }
    }
    let partial = facelets.contains(&8);
    // Any ignored sticker makes its entire corner or edge ignored by the solver.
    let mut visible = [true; 72];
    for piece in CORNER_FACELETS
        .iter()
        .map(|p| p.as_slice())
        .chain(EDGE_FACELETS.iter().map(|p| p.as_slice()))
    {
        if piece.iter().any(|&slot| facelets[slot] == 8) {
            for &slot in piece {
                visible[slot] = false;
            }
        }
    }
    for color in 0..8 {
        let count = facelets
            .iter()
            .enumerate()
            .filter(|&(slot, &value)| visible[slot] && value == color)
            .count();
        if (!partial && count != 9) || count > 9 {
            errors.push(ValidationIssue {
                message: format!(
                    "appears {count} times; expected {}9.",
                    if partial { "at most " } else { "" }
                ),
                color: Some(color),
            });
        }
    }
    diagnose_pieces("Corner", &CORNER_FACELETS, facelets, true, &mut errors);
    diagnose_pieces("Edge", &EDGE_FACELETS, facelets, false, &mut errors);
    diagnose_centers("UF", &UF_CENTER_FACELETS, 0, facelets, &mut errors);
    diagnose_centers("RL", &RL_CENTER_FACELETS, 4, facelets, &mut errors);

    let state = match cubie_from_facelets_for_solving(facelets) {
        Ok(cubie) if errors.is_empty() => Some(cubie_to_state(cubie)),
        Err(error) if errors.is_empty() => {
            issue(&mut errors, error);
            None
        }
        _ => None,
    };
    ValidationReport { state, errors }
}

fn diagnose_pieces<const N: usize, const K: usize>(
    name: &str,
    slots: &[[usize; K]; N],
    facelets: &[u8],
    corners: bool,
    errors: &mut Vec<ValidationIssue>,
) {
    let care = care_mask(slots, facelets);
    let mut seen = [None; N];
    let mut legal = true;
    for pos in 0..N {
        if !care[pos] {
            continue;
        }
        let found = (0..N).find(|&piece| {
            (0..K).any(|orientation| {
                (0..K).all(|t| {
                    facelets[slots[pos][(t + orientation) % K]] == (slots[piece][t] / 9) as u8
                })
            })
        });
        match found {
            None => {
                issue(
                    errors,
                    format!(
                        "{name} {} has a sticker combination that cannot form an FTO {}.",
                        pos + 1,
                        name.to_lowercase()
                    ),
                );
                legal = false;
            }
            Some(piece) => {
                if let Some(previous) = seen[piece] {
                    issue(
                        errors,
                        format!(
                            "{name} {} duplicates the piece at {} {}.",
                            pos + 1,
                            name.to_lowercase(),
                            previous + 1
                        ),
                    );
                    legal = false;
                } else {
                    seen[piece] = Some(pos);
                }
            }
        }
    }
    if care.iter().all(|&value| value) {
        for piece in 0..N {
            if seen[piece].is_none() {
                let faces = slots[piece]
                    .iter()
                    .map(|slot| FACE_NAMES[slot / 9])
                    .collect::<Vec<_>>()
                    .join(" / ");
                issue(
                    errors,
                    format!("The {faces} {} is missing.", name.to_lowercase()),
                );
            }
        }
    }
    // Parity is meaningful only after the pieces in this orbit are identified.
    if legal {
        if let Ok((mut perm, ori)) = detect_partial_pieces(slots, facelets, &care) {
            if corners
                && care.iter().all(|&value| value)
                && ori.iter().fold(0, |xor, value| xor ^ (value >> 1)) != 0
            {
                issue(
                    errors,
                    "Corner twists cannot be solved: the total corner orientation is invalid.",
                );
            }
            if repair_partial_parity(&mut perm, &care).is_err() {
                issue(errors, format!("{name} permutation is invalid: two {}s must be swapped, or more pieces must be ignored in a partial state.", name.to_lowercase()));
            }
        }
    }
}

const FACE_NAMES: [&str; 8] = ["U", "F", "BR", "BL", "D", "B", "R", "L"];

fn diagnose_centers(
    name: &str,
    slots: &[usize; 12],
    base: u8,
    facelets: &[u8],
    errors: &mut Vec<ValidationIssue>,
) {
    let partial = slots.iter().any(|&slot| facelets[slot] == 8);
    for (pos, &slot) in slots.iter().enumerate() {
        let color = facelets[slot];
        if color < 8 && !(base..base + 4).contains(&color) {
            errors.push(ValidationIssue {
                message: format!(
                    "at {name} center {} belongs in the other center orbit.",
                    pos + 1
                ),
                color: Some(color),
            });
        }
    }
    for color in base..base + 4 {
        let count = slots
            .iter()
            .filter(|&&slot| facelets[slot] == color)
            .count();
        if (!partial && count != 3) || count > 3 {
            errors.push(ValidationIssue {
                message: format!(
                    "appears on {count} {name} centers; expected {}3.",
                    if partial { "at most " } else { "" }
                ),
                color: Some(color),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn solved() -> Vec<u8> {
        (0..72).map(|slot| (slot / 9) as u8).collect()
    }
    fn messages(report: &ValidationReport) -> Vec<&str> {
        report
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect()
    }

    #[test]
    fn accepts_solved_and_ignored_pieces() {
        let mut facelets = solved();
        assert!(validate(&facelets).state.is_some());
        for &slot in &CORNER_FACELETS[0] {
            facelets[slot] = 8;
        }
        let corner = CORNER_FACELETS[1];
        facelets.swap(corner[0], corner[2]);
        facelets.swap(corner[1], corner[3]);
        let report = validate(&facelets);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.state.is_some());
    }
    #[test]
    fn accepts_move_generated_states_and_partial_versions() {
        use fto_core::moves::{move_cubies, Move};
        let moves = move_cubies();
        let mut cubie = FtoCubie::solved();
        for mv in Move::ALL.into_iter().cycle().take(114) {
            cubie = cubie.compose(&moves[mv.idx()]);
            let mut facelets = solved();
            for pos in 0..6 {
                for t in 0..4 {
                    facelets[CORNER_FACELETS[pos][(t + usize::from(cubie.co[pos]) * 2) % 4]] =
                        (CORNER_FACELETS[usize::from(cubie.cp[pos])][t] / 9) as u8;
                }
            }
            for pos in 0..12 {
                for t in 0..2 {
                    facelets[EDGE_FACELETS[pos][t]] =
                        (EDGE_FACELETS[usize::from(cubie.ep[pos])][t] / 9) as u8;
                }
                facelets[UF_CENTER_FACELETS[pos]] = cubie.uf[pos] / 3;
                facelets[RL_CENTER_FACELETS[pos]] = [4, 5, 7, 6][usize::from(cubie.rl[pos] / 3)];
            }
            let report = validate(&facelets);
            assert!(report.errors.is_empty(), "{mv:?}: {:?}", report.errors);
            assert!(report.state.is_some());
            facelets[CORNER_FACELETS[0][0]] = 8;
            facelets[EDGE_FACELETS[0][0]] = 8;
            facelets[UF_CENTER_FACELETS[0]] = 8;
            let report = validate(&facelets);
            assert!(
                report.errors.is_empty(),
                "partial {mv:?}: {:?}",
                report.errors
            );
            assert!(report.state.is_some());
        }
    }
    #[test]
    fn reports_multiple_colors_and_both_piece_orbits() {
        let mut facelets = solved();
        facelets[CORNER_FACELETS[0][0]] = 1;
        facelets[EDGE_FACELETS[0][0]] = 6;
        let report = validate(&facelets);
        assert!(report.state.is_none());
        for color in [0, 1, 6] {
            assert!(report.errors.iter().any(|e| e.color == Some(color)));
        }
        let text = messages(&report).join("\n");
        assert!(text.contains("Corner 1 has"));
        assert!(text.contains("Edge 1 has"));
    }
    #[test]
    fn reports_both_center_orbits() {
        let mut facelets = solved();
        facelets.swap(UF_CENTER_FACELETS[0], RL_CENTER_FACELETS[0]);
        let report = validate(&facelets);
        let text = messages(&report).join("\n");
        assert!(text.contains("UF center 1 belongs"));
        assert!(text.contains("RL center 1 belongs"));
        assert!(report.errors.len() >= 4);
    }
    #[test]
    fn reports_independent_parity_errors_together() {
        let mut facelets = solved();
        for orbit in [
            CORNER_FACELETS
                .iter()
                .map(|s| s.as_slice())
                .collect::<Vec<_>>(),
            EDGE_FACELETS.iter().map(|s| s.as_slice()).collect(),
        ] {
            for (&a, &b) in orbit[0].iter().zip(orbit[1]) {
                facelets.swap(a, b);
            }
        }
        let c = CORNER_FACELETS[2];
        facelets.swap(c[0], c[2]);
        facelets.swap(c[1], c[3]);
        let report = validate(&facelets);
        let text = messages(&report).join("\n");
        assert!(text.contains("Corner permutation"));
        assert!(text.contains("Edge permutation"));
        assert!(text.contains("Corner twists"));
    }
    #[test]
    fn reports_duplicate_pieces_and_guards_malformed_input() {
        let mut facelets = solved();
        for (&a, &b) in EDGE_FACELETS[1].iter().zip(&EDGE_FACELETS[0]) {
            facelets[a] = facelets[b];
        }
        let text = messages(&validate(&facelets)).join("\n");
        assert!(text.contains("duplicates"));
        assert!(text.contains("is missing"));
        assert!(validate(&[]).state.is_none());
        facelets[0] = 255;
        assert!(messages(&validate(&facelets))
            .join("\n")
            .contains("unknown color"));
    }
}
