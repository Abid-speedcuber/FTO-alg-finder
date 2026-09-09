use crate::{
    moves::{move_cubies, Move, MOVE_COUNT},
    search::{format_solution, SearchConfig, SearchResult},
    FtoCubie,
};
use std::collections::VecDeque;

const UNVISITED: u8 = u8::MAX;
const EDGE_TABLE_PIECES: usize = 5;
const CORNER_TABLE_PIECES: usize = 5;
const MAX_DYNAMIC_TABLES: usize = 6;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialMask {
    pub corners: [bool; 6],
    pub edges: [bool; 12],
    pub uf_centers: [bool; 12],
    pub rl_centers: [bool; 12],
}

impl PartialMask {
    #[must_use]
    pub const fn all() -> Self {
        Self {
            corners: [true; 6],
            edges: [true; 12],
            uf_centers: [true; 12],
            rl_centers: [true; 12],
        }
    }

    #[must_use]
    pub fn is_full(&self) -> bool {
        self.corners.iter().all(|&v| v)
            && self.edges.iter().all(|&v| v)
            && self.uf_centers.iter().all(|&v| v)
            && self.rl_centers.iter().all(|&v| v)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialProblem {
    pub cubie: FtoCubie,
    pub mask: PartialMask,
}

#[must_use]
pub fn solve_partial(problem: &PartialProblem, config: &SearchConfig) -> SearchResult {
    let pruning = DynamicPruning::build(&problem.mask, &config.allowed_moves);
    eprintln!(
        "using {} dynamic partial pruning tables ({:.1} MiB)",
        pruning.tables.len(),
        pruning.bytes() as f64 / (1024.0 * 1024.0)
    );
    for table in &pruning.tables {
        eprintln!("  {}", table.name());
    }

    let mut total = SearchResult {
        solutions: Vec::new(),
        nodes: 0,
    };
    let start_depth = config
        .min_depth
        .max(pruning.heuristic(&problem.cubie));
    for depth in start_depth..=config.max_depth {
        eprintln!("searching depth {depth}...");
        let mut ctx = PartialSearchContext {
            problem,
            pruning: &pruning,
            config,
            moves: move_cubies(),
            commute: move_commutation(),
            path: Vec::with_capacity(depth as usize),
            solutions: Vec::new(),
            nodes: 0,
        };
        ctx.dfs(problem.cubie, depth, None);
        total.nodes += ctx.nodes;
        total.solutions.extend(ctx.solutions);
        if !total.solutions.is_empty() {
            eprintln!("found solution at depth {depth}");
            break;
        }
    }
    total
}

struct PartialSearchContext<'a> {
    problem: &'a PartialProblem,
    pruning: &'a DynamicPruning,
    config: &'a SearchConfig,
    moves: [FtoCubie; MOVE_COUNT],
    commute: [[bool; MOVE_COUNT]; MOVE_COUNT],
    path: Vec<Move>,
    solutions: Vec<Vec<Move>>,
    nodes: u64,
}

impl PartialSearchContext<'_> {
    fn dfs(&mut self, state: FtoCubie, depth_left: u8, last_move: Option<Move>) {
        if self
            .config
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Relaxed))
        {
            return;
        }
        self.nodes += 1;
        if self.pruning.heuristic(&state) > depth_left {
            return;
        }
        if depth_left == 0 {
            if is_partial_solved(&state, &self.problem.mask) {
                self.solutions.push(self.path.clone());
            }
            return;
        }

        for &mv in &self.config.allowed_moves {
            if last_move.is_some_and(|last| should_skip_after(&self.commute, last, mv)) {
                continue;
            }
            let next = state.compose(&self.moves[mv.idx()]);
            if self.pruning.heuristic(&next) > depth_left - 1 {
                continue;
            }
            self.path.push(mv);
            self.dfs(next, depth_left - 1, Some(mv));
            self.path.pop();
            if !self.config.find_all && !self.solutions.is_empty() {
                return;
            }
        }
    }
}

fn is_partial_solved(state: &FtoCubie, mask: &PartialMask) -> bool {
    for pos in 0..6 {
        if mask.corners[pos] && (state.cp[pos] != pos as u8 || state.co[pos] != 0) {
            return false;
        }
    }
    for pos in 0..12 {
        if mask.edges[pos] && state.ep[pos] != pos as u8 {
            return false;
        }
        if mask.uf_centers[pos] && state.uf[pos] / 3 != pos as u8 / 3 {
            return false;
        }
        if mask.rl_centers[pos] && state.rl[pos] / 3 != pos as u8 / 3 {
            return false;
        }
    }
    true
}

struct DynamicPruning {
    tables: Vec<DynamicTable>,
}

impl DynamicPruning {
    fn build(mask: &PartialMask, allowed_moves: &[Move]) -> Self {
        let mut tables = Vec::new();
        let cared_edges = mask
            .edges
            .iter()
            .enumerate()
            .filter_map(|(piece, &care)| care.then_some(piece as u8))
            .collect::<Vec<_>>();
        let cared_corners = mask
            .corners
            .iter()
            .enumerate()
            .filter_map(|(piece, &care)| care.then_some(piece as u8))
            .collect::<Vec<_>>();

        add_piece_tables(
            &mut tables,
            DynamicKind::Edge,
            &cared_edges,
            EDGE_TABLE_PIECES,
            allowed_moves,
        );
        add_piece_tables(
            &mut tables,
            DynamicKind::Corner,
            &cared_corners,
            CORNER_TABLE_PIECES,
            allowed_moves,
        );
        Self { tables }
    }

    fn heuristic(&self, state: &FtoCubie) -> u8 {
        self.tables
            .iter()
            .map(|table| table.value(state))
            .max()
            .unwrap_or(0)
    }

    fn bytes(&self) -> usize {
        self.tables.iter().map(|table| table.table.len()).sum()
    }
}

fn add_piece_tables(
    tables: &mut Vec<DynamicTable>,
    kind: DynamicKind,
    pieces: &[u8],
    width: usize,
    allowed_moves: &[Move],
) {
    if pieces.is_empty() || tables.len() >= MAX_DYNAMIC_TABLES {
        return;
    }
    if pieces.len() <= width {
        tables.push(DynamicTable::build(kind, pieces.to_vec(), allowed_moves));
        return;
    }
    let stride = width;
    for chunk in pieces.chunks(stride) {
        if tables.len() >= MAX_DYNAMIC_TABLES {
            return;
        }
        if chunk.len() >= 2 {
            tables.push(DynamicTable::build(kind, chunk.to_vec(), allowed_moves));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DynamicKind {
    Corner,
    Edge,
}

struct DynamicTable {
    kind: DynamicKind,
    pieces: Vec<u8>,
    table: Vec<u8>,
    transitions: PieceTransitions,
}

impl DynamicTable {
    fn build(kind: DynamicKind, pieces: Vec<u8>, allowed_moves: &[Move]) -> Self {
        let transitions = PieceTransitions::new();
        let size = match kind {
            DynamicKind::Corner => pow_usize(12, pieces.len()),
            DynamicKind::Edge => pow_usize(12, pieces.len()),
        };
        let mut this = Self {
            kind,
            pieces,
            table: vec![UNVISITED; size],
            transitions,
        };
        let solved = this.solved_index();
        this.table[solved] = 0;
        let mut queue = VecDeque::from([solved]);
        while let Some(idx) = queue.pop_front() {
            let depth = this.table[idx];
            for &mv in allowed_moves {
                if let Some(next) = this.move_index(idx, mv) {
                    if this.table[next] == UNVISITED {
                        this.table[next] = depth + 1;
                        queue.push_back(next);
                    }
                }
            }
        }
        this
    }

    fn name(&self) -> String {
        let prefix = match self.kind {
            DynamicKind::Corner => "partial-corner",
            DynamicKind::Edge => "partial-edge",
        };
        format!(
            "{}({}) {:.1} MiB",
            prefix,
            self.pieces
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(","),
            self.table.len() as f64 / (1024.0 * 1024.0)
        )
    }

    fn solved_index(&self) -> usize {
        let mut idx = 0_usize;
        for &piece in &self.pieces {
            let value = match self.kind {
                DynamicKind::Corner => usize::from(piece) * 2,
                DynamicKind::Edge => usize::from(piece),
            };
            idx = idx * 12 + value;
        }
        idx
    }

    fn value(&self, state: &FtoCubie) -> u8 {
        let idx = self.index_of_state(state);
        let value = self.table[idx];
        if value == UNVISITED {
            0
        } else {
            value
        }
    }

    fn index_of_state(&self, state: &FtoCubie) -> usize {
        match self.kind {
            DynamicKind::Corner => {
                let mut pos_of = [0_u8; 6];
                let mut ori_of = [0_u8; 6];
                for pos in 0..6 {
                    let piece = state.cp[pos] as usize;
                    pos_of[piece] = pos as u8;
                    ori_of[piece] = state.co[pos];
                }
                self.pieces.iter().fold(0_usize, |idx, &piece| {
                    idx * 12 + usize::from(pos_of[piece as usize] * 2 + ori_of[piece as usize])
                })
            }
            DynamicKind::Edge => {
                let mut pos_of = [0_u8; 12];
                for pos in 0..12 {
                    pos_of[state.ep[pos] as usize] = pos as u8;
                }
                self.pieces
                    .iter()
                    .fold(0_usize, |idx, &piece| idx * 12 + usize::from(pos_of[piece as usize]))
            }
        }
    }

    fn move_index(&self, idx: usize, mv: Move) -> Option<usize> {
        let mut values = vec![0_u8; self.pieces.len()];
        let mut rest = idx;
        for value in values.iter_mut().rev() {
            *value = (rest % 12) as u8;
            rest /= 12;
        }

        match self.kind {
            DynamicKind::Corner => {
                let mut used = [false; 6];
                let mut out = 0_usize;
                for value in values {
                    let pos = value / 2;
                    let ori = value & 1;
                    if used[pos as usize] {
                        return None;
                    }
                    used[pos as usize] = true;
                    let next = self.transitions.corner[mv.idx()][pos as usize];
                    out = out * 12 + usize::from(next.pos * 2 + (ori ^ next.ori));
                }
                Some(out)
            }
            DynamicKind::Edge => {
                let mut used = [false; 12];
                let mut out = 0_usize;
                for pos in values {
                    if used[pos as usize] {
                        return None;
                    }
                    used[pos as usize] = true;
                    out = out * 12 + usize::from(self.transitions.edge[mv.idx()][pos as usize]);
                }
                Some(out)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct CornerNext {
    pos: u8,
    ori: u8,
}

struct PieceTransitions {
    corner: [[CornerNext; 6]; MOVE_COUNT],
    edge: [[u8; 12]; MOVE_COUNT],
}

impl PieceTransitions {
    fn new() -> Self {
        let moves = move_cubies();
        let mut corner = [[CornerNext { pos: 0, ori: 0 }; 6]; MOVE_COUNT];
        let mut edge = [[0_u8; 12]; MOVE_COUNT];
        for (move_idx, mv) in moves.iter().enumerate() {
            for dst in 0..6 {
                let src = mv.cp[dst] as usize;
                corner[move_idx][src] = CornerNext {
                    pos: dst as u8,
                    ori: mv.co[dst],
                };
            }
            for dst in 0..12 {
                edge[move_idx][mv.ep[dst] as usize] = dst as u8;
            }
        }
        Self { corner, edge }
    }
}

fn pow_usize(base: usize, exp: usize) -> usize {
    (0..exp).fold(1, |acc, _| acc * base)
}

fn should_skip_after(commute: &[[bool; MOVE_COUNT]; MOVE_COUNT], last: Move, current: Move) -> bool {
    last.axis() == current.axis() || (commute[last.idx()][current.idx()] && current.idx() < last.idx())
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

#[must_use]
pub fn format_partial_solutions(result: SearchResult) -> (u64, Vec<String>) {
    (
        result.nodes,
        result
            .solutions
            .iter()
            .map(|solution| format_solution(solution))
            .collect(),
    )
}
