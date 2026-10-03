//! The process boundary: the environment read once into a [`RuntimeContext`],
//! path and program resolution, process helpers, fault points, and the
//! machine probe.
//!
//! [`RuntimeContext`]: context::RuntimeContext

pub mod bins;
pub mod context;
pub mod fault;
pub mod machine;
pub mod paths;
pub mod process;

pub use bins::{BinOverrides, HarnessBins};
pub use context::{
    Bins, EnvSource, HerdrEnv, Inherited, MapEnv, ProcessEnv, RuntimeContext, Settings, WorkerEnv,
};
pub use fault::Faults;
pub use paths::Paths;
