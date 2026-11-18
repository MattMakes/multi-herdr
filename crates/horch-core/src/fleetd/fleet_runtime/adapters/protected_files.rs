//! The protected file reader: the 1 function that reads an untrusted file
//! and refuses a link, a file that is not regular, another owner, a wide
//! mode, a real path outside its root, or a file over its size limit.
//!
//! Requirements: SR-5, SR-6, FM-23. Ticket 07 of fleetd-p5 fills it.
