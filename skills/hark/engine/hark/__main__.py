"""Lets `python -m hark` behave like the `hark` command."""

from hark import cli

if __name__ == "__main__":
    raise SystemExit(cli.main())
