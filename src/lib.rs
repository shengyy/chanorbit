//! chanOrbit: inclusion processing, fractals, strokes, stroke centres, MACD
//! interval power, the structure geometry snapshot, the buy-side stages
//! with their per-bar observation, and multi-level structure facts. Pure
//! computation: no I/O, no clock, no environment lookups. Definitions are owned by
//! `docs/structure-contract.md`.

pub mod analyzer;
pub mod bar;
pub mod candidate;
pub mod centre;
pub mod contract;
pub mod fractal;
pub mod geometry;
pub mod levels;
pub mod levels_child;
pub mod levels_compare;
pub mod macd;
pub mod observe;
pub mod power;
pub mod stage;
pub mod stroke;

pub use analyzer::{Analyzer, Config};
pub use bar::{NewBar, RawBar};
pub use candidate::CandidateStroke;
pub use centre::{Centre, canonical_centres};
pub use fractal::{Fractal, Mark};
pub use geometry::{Geometry, geometry};
pub use observe::{StageFact, observed_stages};
pub use stage::{Stage, StageKind, stages};
pub use stroke::{Direction, Stroke};
