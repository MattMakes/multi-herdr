//! The process boundary: the environment read once into a [`RuntimeContext`],
//! path and program resolution, process helpers, fault points, and the
//! machine probe.
//!
//! [`RuntimeContext`]: context::RuntimeContext

pub mod bins;
pub(crate) mod context;
pub mod fault;
pub mod machine;
pub(crate) mod paths;
pub mod process;

pub use bins::{BinOverrides, HarnessBins};
pub use context::{EnvSource, Inherited, MapEnv, ProcessEnv, RuntimeContext, Settings, WorkerEnv};
pub use fault::Faults;
pub use paths::Paths;
