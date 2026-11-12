export const EVENT_KINDS = [
  "crash",
  "panic",
  "error",
  "update-failed",
  "hook-reinstalled",
  "breaker-tripped",
] as const;

export type EventKind = (typeof EVENT_KINDS)[number];

export interface Report {
  app: string;
  app_version: string;
  platform: "windows" | "macos";
  os_version: string;
  arch: string;
  install_id: string;
  kind: EventKind;
  message: string;
  source_location?: string;
  backtrace?: string[];
  counts?: Record<string, number>;
  log_lines?: string[];
  foreground_app_hash?: string;
}

export type ValidationResult = { ok: true; report: Report } | { ok: false; reason: string };

// The endpoint takes anonymous input, so it accepts only text the app can produce: each kind has
// a short list of message shapes, and every other free-text field has a fixed pattern. Printable
// ASCII only, which also keeps typed Vietnamese out of what is stored.
interface MessageTemplate {
  title: string;
  pattern: RegExp;
}

const SAFE_PANIC_OPENINGS =
  "called `Option::unwrap\\(\\)` on a `None` value|called `Result::unwrap\\(\\)` on an `Err` value|" +
  "index out of bounds|slice index starts at|range end index|range start index|byte index|" +
  "attempt to [a-z ]{1,60}|assertion failed|internal error: entered unreachable code|" +
  "capacity overflow|already borrowed|\\(message withheld\\)";

export const MESSAGE_TEMPLATES: Record<EventKind, MessageTemplate[]> = {
  crash: [
    { title: "native exception", pattern: /^exception 0x[0-9a-f]{1,8} at (oh-key\.exe|other-module)\+0x[0-9a-f]{1,16}$/ },
    { title: "native exception", pattern: /^exception 0x[0-9a-f]{1,8} at 0x[0-9a-f]{1,16}$/ },
    { title: "fatal signal", pattern: /^signal (SIG[A-Z]{3,8}|\d{1,3})$/ },
  ],
  panic: [
    { title: "panic", pattern: new RegExp(`^panic: (${SAFE_PANIC_OPENINGS})$`) },
    { title: "panic", pattern: /^panic: [A-Za-z0-9 _.,'()-]{1,120}$/ },
    { title: "uncaught exception", pattern: /^uncaught exception [A-Za-z0-9_.]{1,80}$/ },
  ],
  error: [{ title: "SendInput came up short", pattern: /^SendInput took \d{1,6} of \d{1,6} events$/ }],
  "update-failed": [
    { title: "update failed", pattern: /^update to \d{1,4}\.\d{1,4}\.\d{1,4} failed: [\x20-\x7e]{1,200}$/ },
    { title: "update rolled back", pattern: /^the new version stopped within minutes of an update[\x20-\x7e]{0,220}$/ },
  ],
  "hook-reinstalled": [{ title: "hook reinstalled", pattern: /^the keyboard hook was reinstalled$/ }],
  "breaker-tripped": [{ title: "edit loop stopped", pattern: /^the circuit breaker stopped an edit loop$/ }],
};

// Log lines the app can send: `<unix seconds> <known opening><ascii text>`, or the marker.
const LOG_LINE_PATTERN =
  /^(\d{1,11} (crash in |update check failed: |report failed: |update-failed: |error: |update to )[\x20-\x7e]{0,260}|(\d{1,11} )?\(line withheld\))$/;
const BACKTRACE_FRAME_PATTERN = /^(oh_key_[a-z0-9_]+(::[A-Za-z0-9_{}]+)*|\$s6oh_key[A-Za-z0-9_]{0,180})$/;
const SOURCE_LOCATION_PATTERN = /^crates\/[A-Za-z0-9_\/.-]{1,150}\.rs:\d{1,6}(:\d{1,6})?$/;
const APP_VERSION_PATTERN = /^\d{1,4}\.\d{1,4}\.\d{1,4}$/;
const OS_VERSION_PATTERN = /^([A-Za-z]{1,10}( \d{1,3}(\.\d{1,6}){0,3})?|\d{1,3}(\.\d{1,6}){1,3})$/;
const ARCH_PATTERN = /^(x86_64|x86|i686|aarch64|arm64)$/;
const COUNT_KEYS = ["count", "frames"];
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const HASH_PATTERN = /^[0-9a-f]{16,64}$/;
const MAX_BACKTRACE_FRAMES = 40;
const MAX_LOG_LINES = 20;
const MAX_LOG_LINE_LENGTH = 300;

const ALLOWED_FIELDS = new Set([
  "app",
  "app_version",
  "platform",
  "os_version",
  "arch",
  "install_id",
  "kind",
  "message",
  "source_location",
  "backtrace",
  "counts",
  "log_lines",
  "foreground_app_hash",
]);

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isMatchingString(value: unknown, pattern: RegExp): value is string {
  return typeof value === "string" && value.length <= 300 && pattern.test(value);
}

function isPatternList(value: unknown, pattern: RegExp, maxItems: number): boolean {
  return Array.isArray(value) && value.length <= maxItems && value.every((item) => isMatchingString(item, pattern));
}

function isCountMap(value: unknown): boolean {
  if (!isPlainObject(value)) {
    return false;
  }
  return Object.entries(value).every(
    ([key, count]) => COUNT_KEYS.includes(key) && Number.isInteger(count) && (count as number) >= 0 && (count as number) <= 1_000_000_000,
  );
}

export function matchMessageTemplate(kind: EventKind, message: string): MessageTemplate | undefined {
  return MESSAGE_TEMPLATES[kind].find((template) => template.pattern.test(message));
}

function isKnownMessage(kind: unknown, message: unknown): boolean {
  if (typeof kind !== "string" || typeof message !== "string" || message.length > 300) {
    return false;
  }
  if (!(EVENT_KINDS as readonly string[]).includes(kind)) {
    return false;
  }
  return matchMessageTemplate(kind as EventKind, message) !== undefined;
}

function firstInvalidField(input: Record<string, unknown>): string | null {
  for (const key of Object.keys(input)) {
    if (!ALLOWED_FIELDS.has(key)) {
      return `unknown field: ${key.slice(0, 40)}`;
    }
  }
  const checks: Array<[string, boolean]> = [
    ["app", input.app === "oh-key"],
    ["app_version", isMatchingString(input.app_version, APP_VERSION_PATTERN)],
    ["platform", input.platform === "windows" || input.platform === "macos"],
    ["os_version", isMatchingString(input.os_version, OS_VERSION_PATTERN)],
    ["arch", isMatchingString(input.arch, ARCH_PATTERN)],
    ["install_id", isMatchingString(input.install_id, UUID_PATTERN)],
    ["kind", typeof input.kind === "string" && (EVENT_KINDS as readonly string[]).includes(input.kind)],
    ["message", isKnownMessage(input.kind, input.message)],
    ["source_location", input.source_location === undefined || isMatchingString(input.source_location, SOURCE_LOCATION_PATTERN)],
    ["backtrace", input.backtrace === undefined || isPatternList(input.backtrace, BACKTRACE_FRAME_PATTERN, MAX_BACKTRACE_FRAMES)],
    ["counts", input.counts === undefined || isCountMap(input.counts)],
    ["log_lines", input.log_lines === undefined || isLogLineList(input.log_lines)],
    ["foreground_app_hash", input.foreground_app_hash === undefined || isMatchingString(input.foreground_app_hash, HASH_PATTERN)],
  ];
  const failed = checks.find(([, valid]) => !valid);
  return failed ? `invalid field: ${failed[0]}` : null;
}

function isLogLineList(value: unknown): boolean {
  return (
    Array.isArray(value) &&
    value.length <= MAX_LOG_LINES &&
    value.every((line) => typeof line === "string" && line.length <= MAX_LOG_LINE_LENGTH && LOG_LINE_PATTERN.test(line))
  );
}

export function validateReport(input: unknown): ValidationResult {
  if (!isPlainObject(input)) {
    return { ok: false, reason: "body must be a JSON object" };
  }
  const problem = firstInvalidField(input);
  if (problem) {
    return { ok: false, reason: problem };
  }
  return { ok: true, report: input as unknown as Report };
}
