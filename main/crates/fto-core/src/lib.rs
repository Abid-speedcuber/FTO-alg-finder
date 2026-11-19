#![doc = "Core data structures and search scaffolding for the FTO solver."]

pub mod acceleration;
mod display;
pub use display::format_bytes;

pub mod bloom;
pub mod coord;
pub mod cubie;
pub mod moves;
pub mod notation;
pub mod partial;
pub mod pruning;
pub mod search;
mod symmetry;
pub mod tables;

pub use coord::FtoCoord;
pub use cubie::FtoCubie;

mod cache_file;

pub mod partial_goal;
