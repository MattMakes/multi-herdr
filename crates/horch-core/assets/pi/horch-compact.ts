// horch fleet compaction for pi (X3, ai_docs/plans/wave3/x3-design.md).
//
// horch loads this file per launch with `pi --extension <path>`. It replaces
// pi's model-written compaction summary with one built from the session
// alone: no model call, no network, no file write, no environment read.
//
// pi still picks the cut: everything after `preparation.firstKeptEntryId`
// (the newest `keepRecentTokens`) stays verbatim. This file summarizes only
// the older part, and it keeps the `/compact` argument (horch's keep-list,
// with the handoff path) word for word.
//
// No imports, so the file does not depend on pi's package names. Only
// erasable TypeScript syntax: node strips the types in the tests.

const MAX_SUMMARY = 16_000;
const TOOL_HEAD_LINES = 3;
const TOOL_TAIL_LINES = 3;
const TOOL_MAX = 400;
const TEXT_MAX = 600;
const USER_HEAD = 300;
const USER_TAIL = 1_200;
const LATER_USER_MAX = 400;
const LATER_USERS = 8;
const COMMANDS = 12;
const COMMAND_MAX = 200;
const COMMITS = 10;
const FILES = 40;
const PREVIOUS_MAX = 4_000;
const TRANSCRIPT_MESSAGES = 20;

type Block = { type?: string; text?: string; name?: string; id?: string; arguments?: any };
type Message = {
  role?: string;
  content?: string | Block[];
  toolCallId?: string;
  toolName?: string;
  isError?: boolean;
};

function blocks(m: Message): Block[] {
  if (typeof m.content === "string") return [{ type: "text", text: m.content }];
  return Array.isArray(m.content) ? m.content : [];
}

/** The text of a message; thinking blocks are left out. */
function textOf(m: Message): string {
  return blocks(m)
    .filter((b) => b && b.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("\n")
    .trim();
}

function oneLine(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

function clip(s: string, max: number): string {
  return s.length <= max ? s : s.slice(0, max) + ` [trimmed ${s.length - max} chars]`;
}

/** The head and the tail of `s`, with a marker for what is left out. */
function clipMiddle(s: string, head: number, tail: number): string {
  if (s.length <= head + tail) return s;
  return s.slice(0, head) + `\n[trimmed ${s.length - head - tail} chars]\n` + s.slice(-tail);
}

/** Trim a tool output to its first and last lines (the trim step). */
function trimOutput(s: string): string {
  const lines = s.split("\n");
  let out = s;
  if (lines.length > TOOL_HEAD_LINES + TOOL_TAIL_LINES) {
    const cut = lines.length - TOOL_HEAD_LINES - TOOL_TAIL_LINES;
    out = [
      ...lines.slice(0, TOOL_HEAD_LINES),
      `[trimmed ${cut} lines]`,
      ...lines.slice(-TOOL_TAIL_LINES),
    ].join("\n");
  }
  return clipMiddle(out, TOOL_MAX / 2, TOOL_MAX / 2);
}

function list(set: unknown): string[] {
  if (set instanceof Set) return [...set].map(String);
  return Array.isArray(set) ? set.map(String) : [];
}

function section(title: string, lines: string[]): string {
  return lines.length === 0 ? "" : `[${title}]\n` + lines.join("\n");
}

function capped(items: string[], max: number): string[] {
  return items.length <= max ? items : [...items.slice(0, max), `(+${items.length - max} more)`];
}

/** The handoff path in `s`, from horch's own words for it. */
function handoffIn(s: string): string | undefined {
  const re = /(?:handoff file|handoff:|COMPACT-READY|full state is in)\s+([^\s"'`]+)/g;
  let found: string | undefined;
  for (const m of s.matchAll(re)) found = m[1].replace(/[.,;:)]+$/, "");
  return found;
}

type Call = { name: string; command?: string; summary: string; error?: boolean };

/** Every tool call, in order, with whether its result failed. */
function calls(messages: Message[]): Call[] {
  const out: Call[] = [];
  const byId = new Map<string, Call>();
  for (const m of messages) {
    if (m.role === "assistant") {
      for (const b of blocks(m)) {
        if (!b || b.type !== "toolCall") continue;
        const args = b.arguments && typeof b.arguments === "object" ? b.arguments : {};
        const command = typeof args.command === "string" ? args.command : undefined;
        const target = command ?? args.path ?? args.file_path ?? args.pattern ?? "";
        const call: Call = {
          name: String(b.name ?? "tool"),
          command,
          summary: clip(oneLine(String(target)), COMMAND_MAX),
        };
        out.push(call);
        if (b.id) byId.set(b.id, call);
      }
    } else if (m.role === "toolResult" && m.toolCallId) {
      const call = byId.get(m.toolCallId);
      if (call) call.error = m.isError === true;
    }
  }
  return out;
}

function transcriptLine(m: Message): string | undefined {
  if (m.role === "user") {
    const t = textOf(m);
    return t ? "user: " + clip(oneLine(t), TEXT_MAX) : undefined;
  }
  if (m.role === "assistant") {
    const parts: string[] = [];
    const t = textOf(m);
    if (t) parts.push(clip(oneLine(t), TEXT_MAX));
    for (const b of blocks(m)) {
      if (b && b.type === "toolCall") {
        const args = b.arguments && typeof b.arguments === "object" ? b.arguments : {};
        const target = args.command ?? args.path ?? args.file_path ?? args.pattern ?? "";
        parts.push(`-> ${b.name}(${clip(oneLine(String(target)), COMMAND_MAX)})`);
      }
    }
    return parts.length ? "assistant: " + parts.join(" ") : undefined;
  }
  if (m.role === "toolResult") {
    const t = textOf(m);
    const tag = `${m.toolName ?? "tool"}${m.isError ? " error" : ""}`;
    return `result (${tag}): ` + (t ? trimOutput(t) : "(no output)");
  }
  return undefined;
}

/** Build the summary. Exported for nothing but clarity: pi calls the hook. */
function buildSummary(preparation: any, instructions: string | undefined): string {
  const messages: Message[] = [
    ...(Array.isArray(preparation.messagesToSummarize) ? preparation.messagesToSummarize : []),
    ...(Array.isArray(preparation.turnPrefixMessages) ? preparation.turnPrefixMessages : []),
  ].filter((m) => m && typeof m === "object" && m.role !== "system");
  const keep = (instructions ?? "").trim();
  const allCalls = calls(messages);

  // Handoff: the argument first, else the newest one a command names.
  let handoff = keep ? handoffIn(keep) : undefined;
  if (!handoff) {
    for (const c of allCalls) {
      const h = c.command ? handoffIn(c.command) : undefined;
      if (h) handoff = h;
    }
  }

  const users = messages.filter((m) => m.role === "user").map(textOf).filter(Boolean);
  const goal: string[] = [];
  if (users.length > 0) goal.push(clipMiddle(users[0], USER_HEAD, USER_TAIL));
  for (const u of users.slice(1).slice(-LATER_USERS)) goal.push("- " + clip(oneLine(u), LATER_USER_MAX));

  const ops = preparation.fileOps ?? {};
  const read = list(ops.read);
  const modified = [...new Set([...list(ops.written), ...list(ops.edited)])];
  const files: string[] = [];
  if (modified.length) files.push("modified: " + capped(modified, FILES).join(", "));
  if (read.length) files.push("read: " + capped(read, FILES).join(", "));

  const bash = allCalls.filter((c) => c.command !== undefined);
  const commits = bash
    .filter((c) => /\bgit\b[^\n]*\bcommit\b/.test(c.command as string))
    .slice(-COMMITS)
    .map((c) => "- " + c.summary + (c.error ? " (error)" : ""));
  const commands = bash
    .slice(-COMMANDS)
    .map((c) => "- " + c.summary + (c.error ? " (error)" : ""));

  const previous =
    typeof preparation.previousSummary === "string" && preparation.previousSummary.trim()
      ? [clipMiddle(preparation.previousSummary.trim(), PREVIOUS_MAX / 2, PREVIOUS_MAX / 2)]
      : [];

  const head = [
    section("Keep", keep ? [keep] : []),
    section("Handoff", handoff ? [`Read ${handoff} first.`] : []),
    section("Goal", goal),
    section("Files", files),
    section("Commits", commits),
    section("Commands", commands),
    section("Previous summary", previous),
  ].filter(Boolean);

  // The transcript tail fills what is left, newest lines first.
  const lines = messages
    .slice(-TRANSCRIPT_MESSAGES)
    .map(transcriptLine)
    .filter((l): l is string => !!l);
  const fixed = "horch compaction summary (no model call). Older turns, summarized:\n\n" + head.join("\n\n");
  let room = MAX_SUMMARY - fixed.length - "\n\n[Transcript]\n".length;
  const kept: string[] = [];
  for (let i = lines.length - 1; i >= 0 && room > 0; i--) {
    const line = lines[i].length + 1 <= room ? lines[i] : lines[i].slice(-(room - 1));
    kept.unshift(line);
    room -= line.length + 1;
  }
  const summary = kept.length ? fixed + "\n\n[Transcript]\n" + kept.join("\n") : fixed;
  if (summary.length <= MAX_SUMMARY) return summary;
  // Only when the fixed sections alone are too long: the keep-list and the
  // handoff come first, so the cut takes the oldest sections' ends.
  return summary.slice(0, MAX_SUMMARY);
}

export default function horchCompact(pi: any): void {
  pi.on("session_before_compact", (event: any) => {
    try {
      const preparation = event?.preparation;
      if (!preparation || !preparation.firstKeptEntryId) return undefined;
      const summary = buildSummary(preparation, event.customInstructions);
      return {
        compaction: {
          summary,
          firstKeptEntryId: preparation.firstKeptEntryId,
          tokensBefore: preparation.tokensBefore,
          details: { compactor: "horch", version: 1 },
        },
      };
    } catch {
      // pi then writes its own summary: a worse summary, not a lost one.
      return undefined;
    }
  });
}
