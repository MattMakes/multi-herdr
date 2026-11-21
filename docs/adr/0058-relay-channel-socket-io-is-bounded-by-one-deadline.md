# Relay channel socket I/O is bounded by 1 deadline per request

A request to the relay over its Unix socket
(`crates/horch-core/src/messaging/channel.rs`) runs under 1 deadline that
covers the connect, every write and the read of the answer (#67).
`std::os::unix::net::UnixStream::connect` has no timeout, and it can block
while the relay's listen backlog is full. So `connect_by` connects a
nonblocking socket and waits for it only until the deadline. No helper
thread outlives the request.

## Consequences

- A blocking socket write keeps going while a slow reader frees space, past
  any write timeout (macOS). A trickling relay could stretch a send without
  limit. So the stream from `connect_by` stays nonblocking, and `write_by`
  uses `poll` to wait only for the time that is left.
