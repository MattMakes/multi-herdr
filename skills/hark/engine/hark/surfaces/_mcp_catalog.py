"""The fixed catalog of hark MCP tools: names, backing functions, hints and texts.

Pure data plus two small readers; importing it loads nothing heavy.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

REGISTER_HINT = "claude mcp add hark --scope user -e HARK_THREADS=3 -- hark mcp"


@dataclass(frozen=True)
class Tool:
    """One MCP tool: its name, the function behind it, and its hints."""

    name: str
    module: str
    func: str
    read_only: bool
    network: bool


TOOLS: tuple[Tool, ...] = (
    Tool("measure", "hark.core.features", "measure", True, False),
    Tool("describe", "hark.listen.describe", "describe", True, False),
    Tool("match", "hark.listen.match", "match", True, False),
    Tool("rank", "hark.listen.search", "rank", True, False),
    Tool("find", "hark.listen.search", "find", True, False),
    Tool("compare", "hark.listen.compare", "compare", True, False),
    Tool("ab", "hark.listen.compare", "ab", True, False),
    Tool("realness", "hark.listen.realness", "realness", True, False),
    Tool("timeline", "hark.timeline.timeline", "timeline", True, False),
    Tool("sheet", "hark.timeline.sheet", "sheet", False, False),
    Tool("critique", "hark.timeline.critique", "critique", True, False),
    Tool("doctor", "hark.doctor", "check", True, False),
    Tool("llm_critique", "hark.surfaces.llm", "critique", False, True),
    Tool("sfx_list", "hark.forge.bank.sfx", "sfx_list", True, False),
    Tool("sfx_render", "hark.forge.bank.sfx", "sfx_render", False, False),
    Tool("kit_render", "hark.forge.bank.kit", "render", False, False),
)
TOOL_BY_NAME = {t.name: t for t in TOOLS}

DESCRIPTIONS: dict[str, str] = {
    "measure": """Plain meter readings of 1 audio or video file, with no models, so it is quick. It reports length,
        integrated LUFS, true peak in dBTP, loudness range, noise floor, the count of clipped samples, the share of
        exact digital zeros, attack and decay times, brightness, bandwidth, band energies and onsets; music=true
        also estimates tempo, beat times and key. The values are measurements and can be trusted. Use it for
        loudness, peak, clipping and silence checks, and to compare levels before and after a mix edit.""",
    "describe": """What a sound is: calibrated tags grouped by category (for example source, material, action and
        character), the top AudioSet classes, the main measurements and a short paragraph. Use it first on an
        unknown file, or to confirm that a downloaded asset matches its title. It needs the CLAP and AudioSet
        models, so the first call is slow (20 to 60 s); later calls are quick.""",
    "match": """How well 1 sound fits 1 or more text descriptions. It returns z with its verdict band (strong at
        z >= 3, plausible at z >= 2.5, weak at z >= 2, else no match), the percentile among the vocabulary,
        p_vs_alts (a forced choice among the alternatives you pass, not calibrated), the raw cosine (not a
        probability) and the nearest vocabulary terms. Judge by z and the verdict. Pass believable alternatives.
        The first call loads CLAP (20 to 60 s).""",
    "rank": """Orders candidate files by calibrated fit to a text description. Paths can be files, folders or
        globs. Next steps: find on the best file, then describe and realness to confirm it. Embeddings are
        cached, so a second query over the same files is quick; the first call loads CLAP (20 to 60 s).""",
    "find": """The best moments for a description inside a long recording, as ranked segments with start, end, z,
        verdict, kind, head_db, tail_db and cut, in seconds. Event segments begin 5 ms ahead of the attack and
        close after the decay. Texture windows snap their edges to the sound when the query names a one-shot.
        cut is 'head cut' or 'tail cut' when a segment edge falls inside a sound, and such segments rank lower.
        The first call loads CLAP (20 to 60 s).""",
    "compare": """Places 1 sound against a set of reference files: the closest references, a typicality z (>= -1
        belongs to the set, < -2 is an outlier) and the measured differences in plain words. The first call loads
        CLAP (20 to 60 s).""",
    "ab": """Says which of 2 sounds fits a text or a reference set better, and how they differ in measured terms
        (level, brightness, decay length). The same file twice with different trims is allowed. For files with
        several takes, pass trims: a whole-file comparison includes every take and the gaps. The first call loads
        CLAP (20 to 60 s).""",
    "realness": """Real recording or professionally produced audio versus simple code-synthesized sound. It returns
        p_real (a probability), a verdict and the evidence in words; a file longer than 6 s also gets verdicts per
        window, and a folder or glob gives 1 row per file. The target is simple procedural audio, so electronic
        music and professional sound design count as real. Synthetic sound can be the goal: report the verdict,
        and call it a fault only when real sound is wanted. The result is strong evidence, not proof.""",
    "timeline": """A transcript of a whole soundtrack or film: events with time, level, tags and AudioSet classes,
        the loudness curve, picture cuts when there is video, and sync statistics against cuts and cues. A video
        file supplies its own picture unless video names another file. It loads the models on first use (20 to
        60 s); long films take longer.""",
    "sheet": """Draws 1 annotated picture of a soundtrack on a shared time axis: video frames, sections, labelled
        events, a spectrogram, the waveform and loudness, beats, and cue markers whose colour shows sync (green
        inside 1 frame, amber inside 100 ms, red for no hit); cuts are dashed. start and end zoom in. It writes a
        PNG and returns it as an image; use timeline to confirm details. It loads the models on first use.""",
    "critique": """Scores every soundtrack metric (for example loudness, true peak, dynamics, spectral balance,
        event density, sync and realness) against the range of professional launch films. Each metric has a
        status (ok, low, high), a weight, the reason it matters and timed actions, plus 1 overall verdict:
        launch-grade, close, needs work or not launch-grade. Run it before delivery and after each fix round.
        fast=true skips the AudioSet and realness passes.""",
    "doctor": """Checks the hark install: packages, ffmpeg, both models (pinned revision, cache state, offline
        flag), CPU threads and whether the LLM provider (agy) is ready. Run it after a tool failure, or once
        before first use. load=true also loads both models and times 1 window.""",
    "llm_critique": """An optional verbal review by an audio language model (through the agy CLI) that covers sync,
        realism, balance, loudness, cheap or synthetic moments and timed fixes. The audio leaves the machine, so
        set consent=true only after the user said yes. Without a ready provider or without consent nothing is
        sent, and the result explains how to enable it. It is opinion with coarse timing: confirm each fix with
        timeline or ab. A call can take several minutes.""",
    "sfx_list": """Lists the sounds of hark's shipped bank, with length, peak time, kinds and the knobs each sound
        takes (dur, apex, pitch). No models; instant.""",
    "sfx_render": """Writes 1 bank sound as a 24-bit WAV in about 1 s. The length equals dur; apex sets the time of
        the loudest point of a whoosh; pitch changes the recipe's own pitch parameter and never resamples. The
        result has the measured onset, peak and end and the model-free audit.""",
    "kit_render": """Renders a film's sound kit: the takes of every role and a lock with the measured onset, peak,
        end and loudness of each take. The cache key is the sound, seed, edits and bank version, so an unchanged
        kit renders nothing. A take that fails the audit gets a new seed and renders again. No models.""",
}

INSTRUCTIONS = """You cannot hear, so use these hark tools whenever you must judge, choose or check audio: effects,
music, ambience, mixes, or the soundtrack of a video. Pass absolute paths.

Typical chains:
- choosing an asset: rank across the candidates, find inside the winner, describe to confirm, and realness too
  when real or professionally produced sound is wanted;
- reviewing a mix or a film: timeline, sheet, critique;
- checking an edit: ab between the old and the new render, then timeline on the new render;
- plain meters without models: measure;
- llm_critique is optional, uploads the audio and needs the user's explicit yes.

Reading the numbers: z is computed against a few hundred vocabulary phrases on the same sound; strong is
z >= 3, plausible z >= 2.5, weak z >= 2, else no match. p_real and AudioSet scores are probabilities. p_vs_alts
is a forced choice, uncalibrated and only as useful as the alternatives. Cosine is raw CLAP similarity and is
comparable only for 1 sound and 1 query. Trust measurements most, then verdicts and z, and cosine least.

When realness cannot run, report that real versus synthetic is unchecked; the synth classes of describe are
weaker evidence.

The first call that loads a model takes 20 to 60 s; later calls are quick.

Film sounds without models: sfx_list for the bank, then sfx_render for 1 take aimed at a cue (dur, apex,
pitch), or kit_render for a whole kit (takes plus a lock with the measured onset, peak and end)."""



def description(name: str) -> str:
    """A tool's description on one line, runs of whitespace folded to one space."""
    return " ".join(DESCRIPTIONS[name].split())


def _lead(text: str) -> str:
    """Text up to the first full stop that ends a sentence, with the stop kept."""
    found = re.search(r"\.\s", text)
    lead = text if found is None else text[: found.start() + 1]
    return lead if lead.endswith(".") else lead + "."


def tool_list() -> dict[str, Any]:
    """All tools as ``{summary, tools, text}``; ``hark mcp --tools`` prints it."""
    entries = []
    for spec in TOOLS:
        entries.append({"name": spec.name, "read_only": spec.read_only, "network": spec.network,
                        "description": description(spec.name)})
    banner = f"hark MCP server: {len(entries)} tools. Register it with: {REGISTER_HINT}"
    listing = [banner]
    listing.extend(f"{e['name']:<13}{_lead(e['description'])}" for e in entries)
    return {"summary": banner, "tools": entries, "text": "\n".join(listing)}
