//! How panes learn about each other and talk: the worker brief, the mailbox
//! registry, what a message says and how it is typed into a pane.

pub mod brief;
pub mod channel;
pub mod delivery;
pub mod mail;
pub mod mailbox;
pub mod message;
mod private;
