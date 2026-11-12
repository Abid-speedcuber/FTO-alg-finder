#[repr(C)]
struct Timespec { seconds: i64, nanos: i64 }
unsafe extern "C" { fn clock_gettime(clock: i32, time: *mut Timespec) -> i32; }
fn cpu_ms() -> f64 {
 let mut t=Timespec{seconds:0,nanos:0};
 // Linux CLOCK_PROCESS_CPUTIME_ID. A live, correctly laid-out output buffer.
 assert_eq!(unsafe{clock_gettime(2,&mut t)},0);
 t.seconds as f64*1000.+t.nanos as f64/1_000_000.
}
use fto_core::{FtoCubie,moves::Move,pruning::{CandidateSpec,Component,PatternDatabase,PatternDatabases,SolverPruning},tables::TransitionTables,search::{self,SearchConfig}};
use std::{time::Instant,hash::{Hash,Hasher},collections::hash_map::DefaultHasher,sync::{Arc,atomic::{AtomicBool,Ordering}},io::{self,Write}};
const ROOT:&str="/home/abid/Documents/GitHub/FTO/Optimal-FTO-Solver/main";
const P20:&str="/tmp/fto-restart-20261112/profile-data/dev.abid.optimal-fto-solver/tables/pruning";
const P6:&str="/home/abid/.local/share/dev.abid.optimal-fto-solver/tables/pruning";
fn m20()->Vec<Move>{vec![Move::U,Move::Up,Move::F,Move::Fp,Move::BR,Move::BRp,Move::BL,Move::BLp,Move::D,Move::Dp,Move::B,Move::Bp,Move::R,Move::Rp,Move::L,Move::Lp,Move::Fw,Move::Fwp,Move::Rw,Move::Rwp]}
fn seq(s:&[Move])->FtoCubie{s.iter().fold(FtoCubie::solved(),|state,&mv|state.apply(mv))}
fn main(){
 let args:Vec<_>=std::env::args().collect();let family=args.get(1).map(String::as_str).unwrap_or("ll6");let rounds:usize=args.get(2).map(|s|s.parse().unwrap()).unwrap_or(1);let calibrate=args.get(3).is_some_and(|s|s=="calibrate");
 let tables=TransitionTables::load(format!("{ROOT}/cache/transition-table.tran")).unwrap();
 let full=family.starts_with("full");
 let allowed=if family=="ll6" || family=="full6" {vec![Move::U,Move::Up,Move::F,Move::Fp,Move::Fw,Move::Fwp]} else {m20()};
 let solver=if full {Some(SolverPruning::load_or_build_with_moves(&tables,std::path::Path::new(if family=="full6"{P6}else{P20}),0,&allowed).unwrap())}else{None};
 let patterns=if !full{let (path,code)=if family=="ll6"{(P6,"4c82")}else{(P20,"b4ab")};Some(PatternDatabases::new(vec![PatternDatabase::load(CandidateSpec::new(vec![Component::Edge3,Component::LlUfCenterA]),format!("{path}/last-layer-v1/edge3__llufA_{code}.prun")).unwrap(),PatternDatabase::load(CandidateSpec::new(vec![Component::Corner,Component::LlUfCenterB]),format!("{path}/last-layer-v1/corner__llufB_{code}.prun")).unwrap()]))}else{None};
 let mut cases:Vec<(String,FtoCubie,u8)>=vec![];
 if !full{
  let mut photo=FtoCubie::solved();photo.rl.swap(6,9);photo.rl.swap(7,11);cases.push(("photo-four-centres".into(),photo,12));
  for (i,pairs) in [[(6,9),(7,11)],[(6,9),(4,6)],[(4,9),(5,11)],[(4,6),(5,7)]].into_iter().enumerate().skip(1){let mut s=FtoCubie::solved();for(a,b)in pairs{s.rl.swap(a,b)}cases.push((format!("rl-centres-{i}"),s,11));}
  let mut uf=FtoCubie::solved();uf.uf.swap(0,3);uf.uf.swap(1,4);cases.push(("uf-centres".into(),uf,11));
  let mut cp=FtoCubie::solved();cp.cp.swap(0,1);cp.cp.swap(2,3);cases.push(("corners".into(),cp,10));
  let mut ep=FtoCubie::solved();ep.ep.swap(0,1);ep.ep.swap(6,7);cases.push(("edges".into(),ep,10));
  cases.push(("auf-mixed-7".into(),seq(&[Move::F,Move::U,Move::Fp,Move::U,Move::F,Move::U,Move::Fp]),7));
 }
 for seed in 0..6u64{
  let mut random=20261112+seed*7919;let mut state=FtoCubie::solved();let mut last=None;
  let len=if full{10+(seed%3)as u8}else{7+(seed%3)as u8};
  for _ in 0..len{loop{random=random.wrapping_mul(6364136223846793005).wrapping_add(1);let mv=allowed[(random>>32)as usize%allowed.len()];if last.is_none_or(|m:Move|m.idx()/2!=mv.idx()/2){state=state.apply(mv);last=Some(mv);break}}}
  cases.push((format!("mixed-{seed}"),state,len));
 }
 // A baseline-only calibration chooses fixed depths before any variant timings.
 let depth_file=format!("/tmp/fto-ablation-results-20261112/depths-{family}.json");
 let existing=if !calibrate {std::fs::read_to_string(&depth_file).ok()}else{None};
 if let Some(text)=existing{for(name,_,depth)in &mut cases{let marker=format!("\"{name}\":");if let Some(p)=text.find(&marker){let digits=text[p+marker.len()..].trim_start().chars().take_while(char::is_ascii_digit).collect::<String>();*depth=digits.parse().unwrap();}}}
 let mut chosen=vec![];
 for (name,state,mut depth) in cases{
  if family=="full6" {depth=16;}
  if let Some(filter)=args.get(4){if !name.contains(filter){continue}}
  if calibrate{
   let mut last_good=None;
   loop{
    let cancel=Arc::new(AtomicBool::new(false));let finished=Arc::new(AtomicBool::new(false));let c=cancel.clone();let f=finished.clone();
    let watcher=std::thread::spawn(move||{for _ in 0..100 {if f.load(Ordering::Relaxed){return}std::thread::sleep(std::time::Duration::from_millis(20));}c.store(true,Ordering::Relaxed);});
    let config=SearchConfig{min_depth:depth,max_depth:depth,allowed_moves:allowed.clone(),free_u_ends:!full,cancel:Some(cancel.clone()),..SearchConfig::default()};
    let cpu_start=cpu_ms();let now=Instant::now();let r=if full{search::solve_with_pruning_threads(state.coord(),&tables,solver.as_ref(),&config,1)}else{search::solve_last_layer_with_pattern_pruning_threads(state,&tables,patterns.as_ref(),&config,1)};let ms=now.elapsed().as_secs_f64()*1000.;let _cpu=cpu_ms()-cpu_start;finished.store(true,Ordering::Relaxed);watcher.join().unwrap();
    if cancel.load(Ordering::Relaxed) && depth>1 {if let Some(d)=last_good{depth=d;break}depth-=1;continue}
    if ms<200. && r.nodes>3 && depth<20 {last_good=Some(depth);depth+=1;continue}
    eprintln!("CALIBRATE {family} {name} depth={depth} ms={ms:.2} nodes={} algs={}",r.nodes,r.solutions.len());break
   }
   chosen.push(format!("\"{name}\":{depth}"));continue
  }
  let config=SearchConfig{min_depth:depth,max_depth:depth,allowed_moves:allowed.clone(),free_u_ends:!full,..SearchConfig::default()};
  for round in 0..rounds{
   let cpu_start=cpu_ms();let now=Instant::now();let r=if full{search::solve_with_pruning_threads(state.coord(),&tables,solver.as_ref(),&config,1)}else{search::solve_last_layer_with_pattern_pruning_threads(state,&tables,patterns.as_ref(),&config,1)};let ms=now.elapsed().as_secs_f64()*1000.;let cpu=cpu_ms()-cpu_start;let mut h=DefaultHasher::new();r.solutions.hash(&mut h);
   if round==0 {let tag=std::env::var("FTO_BENCH_VARIANT").unwrap_or("unknown".into());let path=format!("/tmp/fto-ablation-results-20261112/algs/{tag}-{family}-{name}.txt");std::fs::create_dir_all("/tmp/fto-ablation-results-20261112/algs").unwrap();std::fs::write(path,r.solutions.iter().map(|a|search::format_solution(a)).collect::<Vec<_>>().join("\n")).unwrap();}
   println!("{{\"family\":\"{family}\",\"case\":\"{name}\",\"depth\":{depth},\"round\":{round},\"ms\":{ms:.6},\"cpu_ms\":{cpu:.6},\"nodes\":{},\"solutions\":{},\"hash\":\"{}\"}}",r.nodes,r.solutions.len(),h.finish());io::stdout().flush().unwrap();
  }
 }
 if calibrate{std::fs::write(depth_file,format!("{{{}}}",chosen.join(","))).unwrap();}
}
