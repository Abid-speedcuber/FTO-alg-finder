use super::SearchResult;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

/// Searches only the requested exact depths, in ascending order. The callback
/// prepares a single depth using tables shared by the enclosing operation.
pub fn search_selected_depths(
    depths: &[u8],
    find_all: bool,
    cancel: Option<&AtomicBool>,
    mut search: impl FnMut(u8) -> Result<SearchResult, String>,
) -> Result<SearchResult, String> {
    let mut depths = depths.to_vec();
    depths.sort_unstable();
    depths.dedup();
    let mut total = SearchResult {
        nodes: 0,
        solutions: Vec::new(),
    };
    let mut seen = HashSet::new();
    for depth in depths {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            break;
        }
        let result = search(depth)?;
        total.nodes = total.nodes.saturating_add(result.nodes);
        for solution in result.solutions {
            if seen.insert(solution.clone()) {
                total.solutions.push(solution);
            }
            if !find_all && !total.solutions.is_empty() {
                break;
            }
        }
        if !find_all && !total.solutions.is_empty() {
            break;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moves::Move;
    #[test]
    fn visits_all_selected_depths_once_and_collects_every_solution() {
        let mut visited = Vec::new();
        let result = search_selected_depths(&[7, 2, 5, 2], true, None, |depth| {
            visited.push(depth);
            Ok(SearchResult {
                nodes: depth as u64,
                solutions: if depth == 2 {
                    vec![]
                } else {
                    vec![vec![Move::R; depth as usize], vec![Move::U; depth as usize]]
                },
            })
        })
        .unwrap();
        assert_eq!(visited, [2, 5, 7]);
        assert_eq!(result.nodes, 14);
        assert_eq!(result.solutions.len(), 4);
    }
    #[test]
    fn stops_at_first_successful_selected_depth_and_returns_one() {
        let mut visited = Vec::new();
        let result = search_selected_depths(&[9, 4, 2, 7], false, None, |depth| {
            visited.push(depth);
            Ok(SearchResult {
                nodes: 10,
                solutions: if depth < 4 {
                    vec![]
                } else {
                    vec![vec![Move::R], vec![Move::U]]
                },
            })
        })
        .unwrap();
        assert_eq!(visited, [2, 4]);
        assert_eq!(result.nodes, 20);
        assert_eq!(result.solutions, [vec![Move::R]]);
    }
    #[test]
    fn cancellation_keeps_results_and_does_not_start_later_depths() {
        let cancel = AtomicBool::new(false);
        let mut visited = Vec::new();
        let result = search_selected_depths(&[2, 3, 4], true, Some(&cancel), |depth| {
            visited.push(depth);
            cancel.store(true, Ordering::Relaxed);
            Ok(SearchResult {
                nodes: 12,
                solutions: vec![vec![Move::R]],
            })
        })
        .unwrap();
        assert_eq!(visited, [2]);
        assert_eq!(result.solutions.len(), 1);
        assert_eq!(result.nodes, 12);
        assert!(
            search_selected_depths(&[1], true, Some(&cancel), |_| panic!(
                "cancelled callback ran"
            ))
            .unwrap()
            .solutions
            .is_empty()
        );
    }
    #[test]
    fn partial_search_reuses_preparation_and_honors_disjoint_depths() {
        use crate::{
            FtoCubie,
            partial::{PartialMask, PartialProblem, PreparedPartialSearch},
            search::SearchConfig,
        };
        let mut mask = PartialMask::all();
        mask.corners.fill(false);
        mask.edges.fill(false);
        mask.uf_centers.fill(false);
        mask.rl_centers.fill(false);
        let problem = PartialProblem {
            cubie: FtoCubie::solved(),
            mask,
        };
        let config = SearchConfig {
            max_depth: 3,
            allowed_moves: vec![Move::R, Move::Rp, Move::U, Move::Up],
            ..Default::default()
        };
        let prepared = PreparedPartialSearch::new(&problem, &config, None);
        for threads in [1, 4] {
            let all = search_selected_depths(&[3, 0, 1], true, None, |depth| {
                Ok(prepared.solve_depth(&config, depth, threads))
            })
            .unwrap();
            let lengths: std::collections::BTreeSet<_> =
                all.solutions.iter().map(Vec::len).collect();
            assert_eq!(lengths, [0, 1, 3].into_iter().collect());
            let single = search_selected_depths(&[3, 1, 0], false, None, |depth| {
                Ok(prepared.solve_depth(
                    &SearchConfig {
                        find_all: false,
                        ..config.clone()
                    },
                    depth,
                    threads,
                ))
            })
            .unwrap();
            assert_eq!(single.solutions, vec![Vec::<Move>::new()]);
        }
    }
}
