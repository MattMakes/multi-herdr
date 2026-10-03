//! Evaluation of competing candidates (phases B3 and B4): mechanical
//! validation first, then the judge: its answer, the strict parser, the
//! rubric and the winner policy.

pub mod judgment;
pub mod parser;
pub mod rubric;
pub mod validator;
pub mod winner;
