# Clean-room specs: comment-pruning

These specs are the only thing that crossed from the source to the writer of `comment-pruning`.

- **Source:** a local source named `comment-removal`, licence class none, sha256 `6df588214ed1`. Imported with `skill-intake`.
- **Spec side:** analyst role workers read the source in a quarantine and wrote these files in their own words. A sanitizer checked each one for copied text and source identifiers. They describe behaviour and hold no text or code from the source.
- **Writer side:** a writer role worker that never opened the source built the skill from these files alone. A reviewer checked the skills against them.
- **Sweep:** [sweep.md](sweep.md) lists every security finding and what happened to it. [provenance.json](provenance.json) records the source.

| Spec | What it covers |
| --- | --- |
| [1-comment-purge-review.md](1-comment-purge-review.md) | Reviewed comment purge with follow-up fixes and constraint encoding |
