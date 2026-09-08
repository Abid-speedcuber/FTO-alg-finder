#![doc = "Core data structures and search scaffolding for the FTO solver."]

pub mod coord;
pub mod cubie;
pub mod moves;
pub mod search;
pub mod tables;

pub use coord::FtoCoord;
pub use cubie::FtoCubie;
