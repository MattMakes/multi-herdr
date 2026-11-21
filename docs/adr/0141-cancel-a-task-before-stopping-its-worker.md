# Cancel a task before stopping its worker

`taskledger::cancel_and_stop` (`crates/horch/src/cmd/taskledger.rs`)
cancels the task first and then stops the worker that held it (TRF-36).
The cancel releases the worker from the task, so the stop that follows
queues nothing again.
