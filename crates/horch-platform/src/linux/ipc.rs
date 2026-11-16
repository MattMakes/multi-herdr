//! The concern ipc for Linux: a Unix stream socket.

pub(crate) use crate::unix::ipc::{
    accept, bind, connect, connect_by, is_not_a_socket, read_line_by, set_nonblocking,
    set_read_timeout, set_timeouts, write_by, Listener, Stream,
};
