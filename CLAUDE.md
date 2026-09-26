# Project rules

## NEVER use ANTHROPIC_API_KEY - EVER

This is an absolute rule from the operator. It has no exceptions.

- Never read, print, set, export, pass or depend on `ANTHROPIC_API_KEY`.
- Never write code, config, docs or teammate files that use it.
- Every `claude` run uses the operator's claude.ai subscription login.
- The operator's shell can have the variable set. Remove it from every
  `claude` command you run: `env -u ANTHROPIC_API_KEY claude ...`.
- If a task seems to need an API key, stop and ask the operator.
