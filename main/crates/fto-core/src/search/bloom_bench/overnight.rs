//! Resumable research worker. Not included in normal solver builds.
use super::*;
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::PathBuf,
    time::Duration,
};

const MAGIC: &[u8; 16] = b"FTO_HLL_CHECK_V1";

#[derive(Clone, Copy)]
struct Frame {
    key: StateKey,
    last: usize,
    next: usize,
}
struct Work {
    depth: usize,
    precision: u32,
    moves: Vec<Move>,
    signature: String,
    registers: Vec<Vec<u8>>,
    paths: Vec<u64>,
    nodes: u64,
    stack: Vec<Frame>,
    elapsed: f64,
}
fn number<T: std::str::FromStr>(name: &str, default: T) -> T
where
    T::Err: std::fmt::Debug,
{
    std::env::var(name)
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(default)
}
fn read_u32(r: &mut impl Read) -> u32 {
    let mut b = [0; 4];
    r.read_exact(&mut b).unwrap();
    u32::from_le_bytes(b)
}
fn read_u64(r: &mut impl Read) -> u64 {
    let mut b = [0; 8];
    r.read_exact(&mut b).unwrap();
    u64::from_le_bytes(b)
}
fn read_u16(r: &mut impl Read) -> u16 {
    let mut b = [0; 2];
    r.read_exact(&mut b).unwrap();
    u16::from_le_bytes(b)
}
fn read_byte(r: &mut impl Read) -> u8 {
    let mut b = [0];
    r.read_exact(&mut b).unwrap();
    b[0]
}

impl Work {
    fn insert(&mut self, key: StateKey, ply: usize) {
        let hash = BloomTable::hashes(key).0;
        let index = (hash >> (64 - self.precision)) as usize;
        let rank = ((hash << self.precision).leading_zeros() + 1).min(65 - self.precision) as u8;
        self.registers[ply][index] = self.registers[ply][index].max(rank);
        self.paths[ply] += 1;
        self.nodes += 1;
    }
    fn save(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        for n in [
            self.depth as u32,
            self.precision,
            self.moves.len() as u32,
            self.signature.len() as u32,
        ] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        bytes.extend(self.moves.iter().map(|mv| mv.idx() as u8));
        bytes.extend_from_slice(self.signature.as_bytes());
        bytes.extend_from_slice(&self.nodes.to_le_bytes());
        bytes.extend_from_slice(&self.elapsed.to_bits().to_le_bytes());
        for &n in &self.paths {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        for row in &self.registers {
            bytes.extend_from_slice(row);
        }
        bytes.extend_from_slice(&(self.stack.len() as u32).to_le_bytes());
        for f in &self.stack {
            for n in [f.key.corner, f.key.edge0, f.key.edge1, f.key.edge3] {
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            bytes.extend_from_slice(&f.key.uf_center.to_le_bytes());
            bytes.extend_from_slice(&f.key.uf3.to_le_bytes());
            bytes.extend_from_slice(&f.key.rl_center.to_le_bytes());
            bytes.push(f.last as u8);
            bytes.extend_from_slice(&(f.next as u32).to_le_bytes());
        }
        let temp = path.with_extension("tmp");
        let mut file = fs::File::create(&temp).unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
        fs::rename(temp, path).unwrap();
    }
    fn load(&mut self, path: &std::path::Path) {
        let bytes = fs::read(path).unwrap();
        let mut r = Cursor::new(bytes.as_slice());
        let mut magic = [0; 16];
        r.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, MAGIC, "checkpoint version mismatch");
        assert_eq!(read_u32(&mut r) as usize, self.depth);
        assert_eq!(read_u32(&mut r), self.precision);
        let count = read_u32(&mut r) as usize;
        let siglen = read_u32(&mut r) as usize;
        assert_eq!(count, self.moves.len());
        assert!(siglen < 4096);
        for mv in &self.moves {
            assert_eq!(read_byte(&mut r) as usize, mv.idx());
        }
        let mut signature = vec![0; siglen];
        r.read_exact(&mut signature).unwrap();
        assert_eq!(
            signature,
            self.signature.as_bytes(),
            "worker build changed; use a new output directory"
        );
        self.nodes = read_u64(&mut r);
        self.elapsed = f64::from_bits(read_u64(&mut r));
        for n in &mut self.paths {
            *n = read_u64(&mut r);
        }
        for row in &mut self.registers {
            r.read_exact(row).unwrap();
        }
        let count = read_u32(&mut r) as usize;
        assert!(count <= self.depth + 1);
        self.stack.clear();
        for _ in 0..count {
            let key = StateKey {
                corner: read_u16(&mut r),
                edge0: read_u16(&mut r),
                edge1: read_u16(&mut r),
                edge3: read_u16(&mut r),
                uf_center: read_u32(&mut r),
                uf3: read_u16(&mut r),
                rl_center: read_u32(&mut r),
            };
            let last = read_byte(&mut r) as usize;
            let next = read_u32(&mut r) as usize;
            assert!(last <= MOVE_COUNT && next <= self.moves.len());
            self.stack.push(Frame { key, last, next });
        }
        assert_eq!(
            r.position() as usize,
            bytes.len(),
            "unexpected checkpoint payload"
        );
        assert_eq!(self.nodes, self.paths.iter().sum::<u64>());
        assert!(
            self.registers
                .iter()
                .flatten()
                .all(|&v| u32::from(v) <= 65 - self.precision)
        );
    }
    fn curves(&self) {
        let mut cumulative = vec![0; 1 << self.precision];
        let mut previous = 0.;
        let mut total = 0_u64;
        for (d, row) in self.registers.iter().enumerate() {
            total += self.paths[d];
            for (a, &b) in cumulative.iter_mut().zip(row) {
                *a = (*a).max(b);
            }
            let n = hll_count(&cumulative);
            println!(
                "WORK_CURVE {{\"depth\":{d},\"words_layer\":{},\"words_total\":{total},\"states\":{n:.6},\"new_states\":{:.6}}}",
                self.paths[d],
                n - previous
            );
            previous = n;
        }
    }
}

#[test]
#[ignore = "resumable overnight curve-counting worker"]
fn overnight_worker() {
    let names = std::env::var("OV_MOVES").unwrap_or_else(|_| "R,U,F".into());
    let depth: usize = number("OV_DEPTH", 8);
    assert!((1..=20).contains(&depth));
    let precision: u32 = number("OV_PRECISION", 18);
    assert!((14..=20).contains(&precision));
    let mut moves = Vec::new();
    for name in names.split(',') {
        let mv = Move::ALL
            .into_iter()
            .find(|mv| format!("{mv:?}") == name)
            .expect("unknown move name");
        for item in [mv, mv.inverse()] {
            if !moves.contains(&item) {
                moves.push(item);
            }
        }
    }
    moves.sort_by_key(|mv| mv.idx());
    let commute = move_commutation();
    let mut successors = vec![Vec::new(); MOVE_COUNT + 1];
    successors[MOVE_COUNT] = moves.clone();
    for &last in &moves {
        successors[last.idx()] = moves
            .iter()
            .copied()
            .filter(|&mv| !should_skip_after(&commute, last, mv))
            .collect();
    }
    let mut expected = vec![1_u128];
    let mut words = vec![1_u128; moves.len()];
    expected.push(moves.len() as u128);
    for _ in 2..=depth {
        let mut next = vec![0_u128; moves.len()];
        for (i, &last) in moves.iter().enumerate() {
            for (j, &mv) in moves.iter().enumerate() {
                if !should_skip_after(&commute, last, mv) {
                    next[j] += words[i];
                }
            }
        }
        words = next;
        expected.push(words.iter().sum());
    }
    println!(
        "WORK_PLAN {{\"moves\":{:?},\"paths\":{:?},\"total\":{},\"precision\":{}}}",
        moves.iter().map(|mv| format!("{mv:?}")).collect::<Vec<_>>(),
        expected,
        expected.iter().sum::<u128>(),
        precision
    );
    if std::env::var_os("OV_PLAN_ONLY").is_some() {
        return;
    }
    assert!(
        expected.iter().sum::<u128>() <= u128::from(u64::MAX),
        "path counter capacity exceeded"
    );
    let tables = TransitionTables::load_or_build("../../cache/transition-table.tran").unwrap();
    let checkpoint = std::env::var_os("OV_CHECKPOINT").map(PathBuf::from);
    let stop_file = std::env::var_os("OV_STOP_FILE").map(PathBuf::from);
    let mut work = Work {
        depth,
        precision,
        moves,
        signature: std::env::var("OV_SIGNATURE").unwrap_or_default(),
        registers: vec![vec![0; 1 << precision]; depth + 1],
        paths: vec![0; depth + 1],
        nodes: 0,
        stack: Vec::new(),
        elapsed: 0.,
    };
    if let Some(path) = checkpoint.as_ref().filter(|p| p.exists()) {
        work.load(path);
        println!(
            "WORK_RESUME nodes={} elapsed_s={:.3}",
            work.nodes, work.elapsed
        );
    } else {
        let key = StateKey::from(SearchState::from_coord(FtoCoord::solved()));
        work.insert(key, 0);
        work.stack.push(Frame {
            key,
            last: MOVE_COUNT,
            next: 0,
        });
    }
    let start = Instant::now();
    let old_elapsed = work.elapsed;
    let initial_nodes = work.nodes;
    let seconds: f64 = number("OV_SECONDS", 3600.);
    assert!(seconds > 0.);
    let checkpoint_seconds: f64 = number("OV_CHECKPOINT_SECONDS", 30.);
    assert!(checkpoint_seconds > 0.);
    let max_new_nodes: u64 = number("OV_MAX_NEW_NODES", u64::MAX);
    let mut last_save = Instant::now();
    let mut last_report = Instant::now();
    let mut paused = false;
    while !work.stack.is_empty() {
        let ply = work.stack.len() - 1;
        let f = work.stack.last_mut().unwrap();
        if ply == depth || f.next == successors[f.last].len() {
            work.stack.pop();
            continue;
        }
        let mv = successors[f.last][f.next];
        f.next += 1;
        let child = f.key.step(&tables, mv);
        work.insert(child, ply + 1);
        work.stack.push(Frame {
            key: child,
            last: mv.idx(),
            next: 0,
        });
        if work.nodes & ((1 << 18) - 1) == 0 || work.nodes - initial_nodes >= max_new_nodes {
            let should_stop = start.elapsed().as_secs_f64() >= seconds
                || stop_file.as_ref().is_some_and(|p| p.exists())
                || work.nodes - initial_nodes >= max_new_nodes;
            if last_report.elapsed() >= Duration::from_secs(5) || should_stop {
                let elapsed = start.elapsed().as_secs_f64();
                let rate = (work.nodes - initial_nodes) as f64 / elapsed.max(0.001);
                println!(
                    "WORK_PROGRESS {{\"nodes\":{},\"total\":{},\"elapsed_s\":{:.3},\"nodes_per_s\":{rate:.3}}}",
                    work.nodes,
                    expected.iter().sum::<u128>(),
                    old_elapsed + elapsed
                );
                last_report = Instant::now();
            }
            if should_stop || last_save.elapsed().as_secs_f64() >= checkpoint_seconds {
                work.elapsed = old_elapsed + start.elapsed().as_secs_f64();
                if let Some(path) = &checkpoint {
                    work.save(path);
                }
                last_save = Instant::now();
            }
            if should_stop {
                paused = true;
                break;
            }
        }
    }
    work.elapsed = old_elapsed + start.elapsed().as_secs_f64();
    if paused {
        println!(
            "WORK_PAUSED nodes={} elapsed_s={:.3}",
            work.nodes, work.elapsed
        );
        return;
    }
    assert_eq!(u128::from(work.nodes), expected.iter().sum::<u128>());
    for (&actual, &planned) in work.paths.iter().zip(&expected) {
        assert_eq!(u128::from(actual), planned);
    }
    work.curves();
    println!(
        "WORK_DONE {{\"nodes\":{},\"elapsed_s\":{:.6},\"standard_error_pct\":{:.6}}}",
        work.nodes,
        work.elapsed,
        100. * 1.04 / ((1_u64 << precision) as f64).sqrt()
    );
}
