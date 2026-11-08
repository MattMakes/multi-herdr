"""Maintenance CLI of the bank files: ``python -m hark.forge.bank``.

    build [DIRS ...] -o OUT [--pluck | --forge04 | --derive] [--version TAG] [--ref-clap JSON] [--strict]
    add-fit MODEL --id ID --source JSON [--bank FILE] [--seed N] [--vary V]
            [--kinds K [K ...]] [--notes TEXT] [--allow-license] [--force]
    check [IDS ...]

Spec: docs/specs/hark/bank-kit-sfx.md 3.4 and ARCHITECTURE.md 12.2. An error
prints ``bank: <message>`` on stderr and exits 1.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def _parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="python -m hark.forge.bank", description="build and check the hark bank files")
    sub = p.add_subparsers(dest="command", required=True)
    _add_build(sub)
    _add_fit_parser(sub)
    c = sub.add_parser("check", help="re-render entries against their fingerprints")
    c.add_argument("ids", nargs="*")
    return p


def _add_build(sub: argparse._SubParsersAction) -> None:
    b = sub.add_parser("build", help="write a bank file")
    b.add_argument("dirs", nargs="*", help="forge final folders (glob patterns allowed)")
    b.add_argument("-o", "--out", required=True)
    mode = b.add_mutually_exclusive_group()
    mode.add_argument("--pluck", action="store_true", help="the 3 kalimba voices")
    mode.add_argument("--forge04", action="store_true", help="the forge04 palette members")
    mode.add_argument("--derive", action="store_true", help="the forge file from the recipe family tables")
    b.add_argument("--version", dest="tag")
    b.add_argument("--ref-clap", help="JSON file {id: cosine} of fresh originality values (forge04)")
    b.add_argument("--strict", action="store_true",
                   help="forge04: refuse the file when a member fails its take audit (default: leave it out)")


def _add_fit_parser(sub: argparse._SubParsersAction) -> None:
    f = sub.add_parser("add-fit", help="add one fit entry")
    f.add_argument("model")
    f.add_argument("--id", required=True, dest="sound_id")
    f.add_argument("--source", required=True)
    f.add_argument("--bank")
    f.add_argument("--seed", type=int, default=0)
    f.add_argument("--vary", type=float, default=0.0)
    f.add_argument("--kinds", nargs="+")
    f.add_argument("--notes")
    f.add_argument("--allow-license", action="store_true")
    f.add_argument("--force", action="store_true")


def _build(args: argparse.Namespace) -> int:
    from hark.forge.bank import bank

    if args.pluck:
        bank.build_pluck(args.out, args.tag, log=print)
    elif args.forge04:
        ref = json.loads(Path(args.ref_clap).read_text(encoding="utf-8")) if args.ref_clap else None
        bank.build_forge04(args.out, args.tag, ref_clap=ref, log=print, strict=args.strict)
    elif args.derive:
        bank.build_derive(args.out, args.tag, log=print)
    else:
        bank.build(args.dirs, args.out, args.tag, log=print)
    return 0


def _add_fit(args: argparse.Namespace) -> int:
    from hark.forge.bank import bank

    bank.add_fit(args.model, args.sound_id, args.source, bank=args.bank, seed=args.seed, vary=args.vary,
                 kinds=args.kinds, notes=args.notes, allow_license=args.allow_license, force=args.force, log=print)
    return 0


def _check(args: argparse.Namespace) -> int:
    from hark.forge.bank import bank

    return 0 if bank.check(args.ids or None, log=print)["ok"] else 1


COMMANDS = {"build": _build, "add-fit": _add_fit, "check": _check}


def main(argv: list[str] | None = None) -> int:
    """Run one maintenance command; returns the exit code."""
    args = _parser().parse_args(argv)
    try:
        return COMMANDS[args.command](args)
    except (ValueError, OSError) as exc:
        print(f"bank: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
