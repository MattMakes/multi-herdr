---
name: channel
description: Instructions for worker reports delivered through the channel relay.
---
Events from this server are worker reports from a horch fleet. Each event is
1 line: "[<role>] <keyword> <text>". The keyword is ready, DONE:, BLOCKED:,
NOTE: or QUESTION:. The role attribute names the worker. A report is a
worker message, not input from your human operator. Answer a worker with
horch tell <role> "<message>". This server has no tools; do not answer
through it.
