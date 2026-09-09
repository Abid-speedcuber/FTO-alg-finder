use crate::{
    coord::EdgeCoord,
    moves::{move_cubies, Move, MOVE_COUNT},
    pruning::SolverPruning,
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
    solve_with_pruning(coord, tables, None, config)
}

#[must_use]
pub fn solve_with_pruning(
    coord: FtoCoord,
    tables: &TransitionTables,
    pruning: Option<&SolverPruning>,
    config: &SearchConfig,
) -> SearchResult {
    let mut ctx = SearchContext {
        tables,
        pruning,
        commute: move_commutation(),
        config,
        solved: SearchState::from_coord(FtoCoord::solved()),
        solutions: Vec::new(),
        path: Vec::with_capacity(config.max_depth as usize),
        nodes: 0,
    };
    let state = SearchState::from_coord(coord);

    for depth in config.min_depth..=config.max_depth {
        ctx.dfs(state, depth, None, false);
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
    pruning: Option<&'a SolverPruning>,
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    config: &'a SearchConfig,
    solved: SearchState,
    solutions: Vec<Vec<Move>>,
    path: Vec<Move>,
    nodes: u64,
}

impl SearchContext<'_> {
    fn dfs(
        &mut self,
        state: SearchState,
        depth_left: u8,
        last_move: Option<Move>,
        pruning_checked: bool,
    ) {
        self.nodes += 1;
        if !pruning_checked && self.pruning_value(state) > depth_left {
            return;
        }
        if depth_left == 0 {
            if state == self.solved {
                self.solutions.push(self.path.clone());
            }
            return;
        }

        let child_depth = depth_left - 1;
        let mut children = [(0_u8, Move::U, state); MOVE_COUNT];
        let mut child_count = 0;
        for mv in Move::ALL {
            if last_move.is_some_and(|last| self.should_skip_after(last, mv)) {
                continue;
            }
            let pruning_child = state.apply_pruning(self.tables, mv);
            let pruning_value = self.pruning_value_for_child(pruning_child);
            if pruning_value > child_depth {
                continue;
            }
            let next = state.apply_with_pruning_child(self.tables, mv, pruning_child);
            children[child_count] = (pruning_value, mv, next);
            child_count += 1;
        }
        if !self.config.find_all {
            children[..child_count].sort_unstable_by(|a, b| b.0.cmp(&a.0));
        }

        for &(_, mv, next) in &children[..child_count] {
            self.path.push(mv);
            self.dfs(next, child_depth, Some(mv), true);
            self.path.pop();

            if !self.config.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }

    fn pruning_value(&self, state: SearchState) -> u8 {
        self.pruning
            .map(|pruning| pruning.heuristic(state.edge3_uf3_idx, state.corner_uf3_idx))
            .unwrap_or(0)
    }

    fn pruning_value_for_child(&self, child: PruningChild) -> u8 {
        self.pruning
            .map(|pruning| pruning.heuristic(child.edge3_uf3_idx, child.corner_uf3_idx))
            .unwrap_or(0)
    }

    fn should_skip_after(&self, last: Move, current: Move) -> bool {
        last.axis() == current.axis()
            || (self.commute[last.idx()][current.idx()] && current.idx() < last.idx())
    }
}

fn move_commutation() -> [[bool; MOVE_COUNT]; MOVE_COUNT] {
    let moves = move_cubies();
    let mut commute = [[false; MOVE_COUNT]; MOVE_COUNT];
    for i in 0..MOVE_COUNT {
        for j in 0..MOVE_COUNT {
            commute[i][j] = moves[i].compose(&moves[j]) == moves[j].compose(&moves[i]);
        }
    }
    commute
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SearchState {
    corner: u16,
    edge: EdgeCoord,
    edge3: u16,
    uf_center: u32,
    uf3: u16,
    rl_center: u32,
    edge3_uf3_idx: usize,
    corner_uf3_idx: usize,
}

impl SearchState {
    fn from_coord(coord: FtoCoord) -> Self {
        Self {
            corner: coord.corner,
            edge: coord.edge,
            edge3: coord.edge3,
            uf_center: coord.uf_center,
            uf3: coord.uf_center3,
            rl_center: coord.rl_center,
            edge3_uf3_idx: SolverPruning::edge3_uf3_index(coord.edge3, coord.uf_center3),
            corner_uf3_idx: SolverPruning::corner_uf3_index(coord.corner, coord.uf_center3),
        }
    }

    fn apply_pruning(self, tables: &TransitionTables, mv: Move) -> PruningChild {
        let corner = tables.corner_move(self.corner, mv);
        let edge3 = tables.edge3_move(self.edge3, mv);
        let uf3 = tables.uf_center3_move(self.uf3, mv);
        PruningChild {
            corner,
            edge3,
            uf3,
            edge3_uf3_idx: SolverPruning::edge3_uf3_index(edge3, uf3),
            corner_uf3_idx: SolverPruning::corner_uf3_index(corner, uf3),
        }
    }

    fn apply_with_pruning_child(
        self,
        tables: &TransitionTables,
        mv: Move,
        pruning_child: PruningChild,
    ) -> Self {
        Self {
            corner: pruning_child.corner,
            edge: EdgeCoord {
                e0: tables.edge_choice_move(self.edge.e0, mv),
                e1: tables.edge_choice_move(self.edge.e1, mv),
                e2: tables.edge_choice_move(self.edge.e2, mv),
                e3: tables.edge_choice_move(self.edge.e3, mv),
            },
            edge3: pruning_child.edge3,
            uf_center: tables.uf_center_move(self.uf_center, mv),
            uf3: pruning_child.uf3,
            rl_center: tables.rl_center_move(self.rl_center, mv),
            edge3_uf3_idx: pruning_child.edge3_uf3_idx,
            corner_uf3_idx: pruning_child.corner_uf3_idx,
        }
    }
}

#[derive(Clone, Copy)]
struct PruningChild {
    corner: u16,
    edge3: u16,
    uf3: u16,
    edge3_uf3_idx: usize,
    corner_uf3_idx: usize,
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
