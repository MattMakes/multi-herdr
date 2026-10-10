//! Fleet context policy: native windows, watch thresholds, row states and
//! the compaction job (design `ai_docs/plans/wave2/w1/design.md` §6).
//!
//! | module | job |
//! |---|---|
//! | [`window`] | windows, native triggers and thresholds (pure, CTX-07) |
//! | [`policy`] | row states, the warn rule, handoffs, the compact line (pure) |
//! | [`job`] | the compaction job's state machine, I/O through [`job::JobPorts`] |
//! | [`jobfile`] | the job dir: `job.json`, `spawned`, `lost`, liveness and start |

pub mod job;
pub mod jobfile;
pub mod policy;
pub mod window;
