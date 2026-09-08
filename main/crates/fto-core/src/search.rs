use crate::{
    moves::{Move, MOVE_COUNT},
    tables::TransitionTables,
    FtoCoord,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchConfig {
    pub min_depth: u8,
    pub max_depth: u8,
    pub find_all: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            min_depth: 0,
            max_depth: 6,
            find_all: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    pub solutions: Vec<Vec<Move>>,
    pub nodes: u64,
}

#[must_use]
pub fn solve(coord: FtoCoord, tables: &TransitionTables, config: &SearchConfig) -> SearchResult {
    let mut ctx = SearchContext {
        tables,
        config,
        solved: FtoCoord::solved(),
        solutions: Vec::new(),
        path: Vec::with_capacity(config.max_depth as usize),
        nodes: 0,
    };

    for depth in config.min_depth..=config.max_depth {
        ctx.dfs(coord, depth, None);
        if !config.find_all && !ctx.solutions.is_empty() {
            break;
        }
    }

    SearchResult {
        solutions: ctx.solutions,
        nodes: ctx.nodes,
    }
}

struct SearchContext<'a> {
    tables: &'a TransitionTables,
    config: &'a SearchConfig,
    solved: FtoCoord,
    solutions: Vec<Vec<Move>>,
    path: Vec<Move>,
    nodes: u64,
}

impl SearchContext<'_> {
    fn dfs(&mut self, coord: FtoCoord, depth_left: u8, last_axis: Option<u8>) {
        self.nodes += 1;
        if depth_left == 0 {
            if coord == self.solved {
                self.solutions.push(self.path.clone());
            }
            return;
        }

        for mv in Move::ALL {
            if last_axis == Some(mv.axis()) {
                continue;
            }
            self.path.push(mv);
            let next = self.tables.apply(coord, mv);
            self.dfs(next, depth_left - 1, Some(mv.axis()));
            self.path.pop();

            if !self.config.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }
}

#[must_use]
pub fn format_solution(solution: &[Move]) -> String {
    solution.iter().map(|mv| mv.name()).collect::<Vec<_>>().join(" ")
}

pub const RAW_BRANCHING_FACTOR: usize = MOVE_COUNT;

#[cfg(test)]
mod tests {
    use crate::{moves::Move, search::SearchConfig, tables::TransitionTables, FtoCubie};

    use super::{format_solution, solve};

    #[test]
    #[ignore = "builds full center transition tables"]
    fn finds_inverse_of_short_scramble() {
        let tables = TransitionTables::build();
        let scrambled = FtoCubie::solved().apply(Move::R).apply(Move::U);
        let result = solve(
            scrambled.coord(),
            &tables,
            &SearchConfig {
                min_depth: 0,
                max_depth: 2,
                find_all: false,
            },
        );

        assert_eq!(format_solution(&result.solutions[0]), "U' R'");
    }
}
