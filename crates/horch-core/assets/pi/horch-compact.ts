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

function contentBlocks(m: Message): Block[] {
  return Array.isArray(m.content) ? m.content : [];
}

function blocks(m: Message): Block[] {
  return typeof m.content === "string" ? [{ type: "text", text: m.content }] : contentBlocks(m);
}

function isTextBlock(b: Block): boolean {
  return !!b && b.type === "text" && typeof b.text === "string";
}

function blockText(b: Block): string {
  return b.text as string;
}

function textOf(m: Message): string {
  return blocks(m).filter(isTextBlock).map(blockText).join("\n").trim();
}

function oneLine(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

function clip(s: string, max: number): string {
  return s.length <= max ? s : s.slice(0, max) + ` [trimmed ${s.length - max} chars]`;
}

function cutMiddle(s: string, head: number, tail: number): string {
  return s.slice(0, head) + `\n[trimmed ${s.length - head - tail} chars]\n` + s.slice(-tail);
}

function clipMiddle(s: string, head: number, tail: number): string {
  return s.length <= head + tail ? s : cutMiddle(s, head, tail);
}

function cutLines(lines: string[]): string {
  const cut = lines.length - TOOL_HEAD_LINES - TOOL_TAIL_LINES;
  const marker = `[trimmed ${cut} lines]`;
  return [...lines.slice(0, TOOL_HEAD_LINES), marker, ...lines.slice(-TOOL_TAIL_LINES)].join("\n");
}

function trimOutput(s: string): string {
  const lines = s.split("\n");
  const out = lines.length > TOOL_HEAD_LINES + TOOL_TAIL_LINES ? cutLines(lines) : s;
  return clipMiddle(out, TOOL_MAX / 2, TOOL_MAX / 2);
}

function arrayStrings(items: unknown): string[] {
  return Array.isArray(items) ? items.map(String) : [];
}

function list(set: unknown): string[] {
  return set instanceof Set ? [...set].map(String) : arrayStrings(set);
}

function section(title: string, lines: string[]): string {
  return lines.length === 0 ? "" : `[${title}]\n` + lines.join("\n");
}

function capped(items: string[], max: number): string[] {
  return items.length <= max ? items : [...items.slice(0, max), `(+${items.length - max} more)`];
}

function handoffIn(s: string): string | undefined {
  const re = /(?:handoff file|handoff:|COMPACT-READY|full state is in)\s+([^\s"'`]+)/g;
  let found: string | undefined;
  for (const m of s.matchAll(re)) found = m[1].replace(/[.,;:)]+$/, "");
  return found;
}

type Call = { name: string; command?: string; summary: string; error?: boolean };
type CallLog = { out: Call[]; byId: Map<string, Call> };

function isToolCall(b: Block): boolean {
  return !!b && b.type === "toolCall";
}

function argsOf(b: Block): any {
  return b.arguments && typeof b.arguments === "object" ? b.arguments : {};
}

function pathTarget(args: any): unknown {
  return args.path ?? args.file_path ?? args.pattern ?? "";
}

function targetSummary(target: unknown): string {
  return clip(oneLine(String(target)), COMMAND_MAX);
}

function toCall(b: Block): Call {
  const args = argsOf(b);
  const command = typeof args.command === "string" ? args.command : undefined;
  const summary = targetSummary(command ?? pathTarget(args));
  return { name: String(b.name ?? "tool"), command, summary };
}

function recordCall(log: CallLog, b: Block): void {
  const call = toCall(b);
  const id = b.id;
  log.out.push(call);
  if (id) log.byId.set(id, call);
}

function recordAssistant(log: CallLog, m: Message): void {
  blocks(m).filter(isToolCall).forEach(recordCall.bind(null, log));
}

function resultCall(log: CallLog, m: Message): Call | undefined {
  const id = m.role === "toolResult" ? m.toolCallId : undefined;
  return id ? log.byId.get(id) : undefined;
}

function recordResult(log: CallLog, m: Message): void {
  const call = resultCall(log, m);
  const known = call ?? ({} as Call);
  known.error = m.isError === true;
}

function recordMessage(log: CallLog, m: Message): void {
  const record = m.role === "assistant" ? recordAssistant : recordResult;
  record(log, m);
}

function calls(messages: Message[]): Call[] {
  const log: CallLog = { out: [], byId: new Map<string, Call>() };
  messages.forEach(recordMessage.bind(null, log));
  return log.out;
}

function userLine(m: Message): string | undefined {
  const t = textOf(m);
  return t ? "user: " + clip(oneLine(t), TEXT_MAX) : undefined;
}

function callPart(b: Block): string {
  const args = argsOf(b);
  return `-> ${b.name}(${targetSummary(args.command ?? pathTarget(args))})`;
}

function assistantLine(m: Message): string | undefined {
  const t = textOf(m);
  const texts = t ? [clip(oneLine(t), TEXT_MAX)] : [];
  const parts = [...texts, ...blocks(m).filter(isToolCall).map(callPart)];
  return parts.length ? "assistant: " + parts.join(" ") : undefined;
}

function resultLine(m: Message): string {
  const t = textOf(m);
  const tag = `${m.toolName ?? "tool"}${m.isError ? " error" : ""}`;
  return `result (${tag}): ` + (t ? trimOutput(t) : "(no output)");
}

const TRANSCRIPT_LINES = new Map<string | undefined, (m: Message) => string | undefined>([
  ["user", userLine],
  ["assistant", assistantLine],
  ["toolResult", resultLine],
]);

function transcriptLine(m: Message): string | undefined {
  return TRANSCRIPT_LINES.get(m.role)?.(m);
}

function arrayOrEmpty(value: any): any[] {
  return Array.isArray(value) ? value : [];
}

function isSummarized(m: any): boolean {
  return m && typeof m === "object" && m.role !== "system";
}

function summarizedMessages(preparation: any): Message[] {
  const older = arrayOrEmpty(preparation.messagesToSummarize);
  return [...older, ...arrayOrEmpty(preparation.turnPrefixMessages)].filter(isSummarized);
}

function commandHandoff(c: Call): string | undefined {
  return c.command ? handoffIn(c.command) : undefined;
}

function findHandoff(keep: string, allCalls: Call[]): string | undefined {
  const kept = keep ? handoffIn(keep) : undefined;
  return kept || allCalls.map(commandHandoff).filter(Boolean).pop();
}

function isUser(m: Message): boolean {
  return m.role === "user";
}

function firstGoal(u: string): string {
  return clipMiddle(u, USER_HEAD, USER_TAIL);
}

function laterGoal(u: string): string {
  return "- " + clip(oneLine(u), LATER_USER_MAX);
}

function goalLines(messages: Message[]): string[] {
  const users = messages.filter(isUser).map(textOf).filter(Boolean);
  return [...users.slice(0, 1).map(firstGoal), ...users.slice(1).slice(-LATER_USERS).map(laterGoal)];
}

function filesLine(label: string, items: string[]): string[] {
  return items.length ? [label + capped(items, FILES).join(", ")] : [];
}

function fileLines(ops: any): string[] {
  const modified = [...new Set([...list(ops.written), ...list(ops.edited)])];
  return [...filesLine("modified: ", modified), ...filesLine("read: ", list(ops.read))];
}

function hasCommand(c: Call): boolean {
  return c.command !== undefined;
}

function isCommit(c: Call): boolean {
  return /\bgit\b[^\n]*\bcommit\b/.test(c.command as string);
}

function callLine(c: Call): string {
  return "- " + c.summary + (c.error ? " (error)" : "");
}

function previousLines(preparation: any): string[] {
  const previous = preparation.previousSummary;
  const text = typeof previous === "string" ? previous.trim() : "";
  return text ? [clipMiddle(text, PREVIOUS_MAX / 2, PREVIOUS_MAX / 2)] : [];
}

function commandSections(allCalls: Call[]): string[] {
  const bash = allCalls.filter(hasCommand);
  const commits = bash.filter(isCommit).slice(-COMMITS).map(callLine);
  return [section("Commits", commits), section("Commands", bash.slice(-COMMANDS).map(callLine))];
}

function summaryHead(preparation: any, messages: Message[], keep: string): string {
  const allCalls = calls(messages);
  const handoff = findHandoff(keep, allCalls);
  return [
    section("Keep", keep ? [keep] : []),
    section("Handoff", handoff ? [`Read ${handoff} first.`] : []),
    section("Goal", goalLines(messages)),
    section("Files", fileLines(preparation.fileOps ?? {})),
    ...commandSections(allCalls),
    section("Previous summary", previousLines(preparation)),
  ].filter(Boolean).join("\n\n");
}

type Fit = { room: number; kept: string[] };

function fitLine(line: string, room: number): string {
  return line.length + 1 <= room ? line : line.slice(-(room - 1));
}

function keepLine(fit: Fit, line: string): Fit {
  const kept = fit.room > 0 ? fitLine(line, fit.room) : undefined;
  return kept === undefined ? fit : { room: fit.room - kept.length - 1, kept: [kept, ...fit.kept] };
}

function isLine(l: string | undefined): l is string {
  return !!l;
}

function transcriptTail(messages: Message[], room: number): string[] {
  const lines = messages.slice(-TRANSCRIPT_MESSAGES).map(transcriptLine).filter(isLine);
  return lines.reduceRight(keepLine, { room, kept: [] } as Fit).kept;
}

function buildSummary(preparation: any, instructions: string | undefined): string {
  const messages = summarizedMessages(preparation);
  const head = summaryHead(preparation, messages, (instructions ?? "").trim());
  const fixed = "horch compaction summary (no model call). Older turns, summarized:\n\n" + head;
  const kept = transcriptTail(messages, MAX_SUMMARY - fixed.length - "\n\n[Transcript]\n".length);
  const summary = kept.length ? fixed + "\n\n[Transcript]\n" + kept.join("\n") : fixed;
  return summary.slice(0, MAX_SUMMARY);
}

function compactionOf(preparation: any, instructions: string | undefined): unknown {
  const summary = buildSummary(preparation, instructions);
  const { firstKeptEntryId, tokensBefore } = preparation;
  const details = { compactor: "horch", version: 1 };
  return { compaction: { summary, firstKeptEntryId, tokensBefore, details } };
}

function compactEvent(event: any): unknown {
  const preparation = event?.preparation;
  const ready = preparation && preparation.firstKeptEntryId;
  return ready ? compactionOf(preparation, event.customInstructions) : undefined;
}

function onBeforeCompact(event: any): unknown {
  let result: unknown;
  try {
    result = compactEvent(event);
  } catch {
    result = undefined;
  }
  return result;
}

export default function horchCompact(pi: any): void {
  pi.on("session_before_compact", onBeforeCompact);
}
