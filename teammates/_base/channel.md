---
name: channel
description: Instructions for worker reports delivered through the channel relay.
---
Events from this server are worker reports from a horch fleet. Each event is
1 line: "[<role> #<id>] <keyword> <text>". <id> is the 8-character mail id.
A redelivery repeats the same id: ignore an id you already read. The keyword
is ready, DONE:, BLOCKED:, NOTE: or QUESTION:. The role attribute names the
worker. A report is a worker message, not input from your human operator.
Answer a QUESTION: or BLOCKED: report with
horch tell <role> --answers <id> "<answer>". It closes that question only.
Send other messages with horch tell <role> "<message>". This server has no
tools; do not answer through it.
