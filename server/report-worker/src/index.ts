import { addComment, commentBody, createIssue, issueBody, issueTitle } from "./github";
import { bumpCounter, dayStamp, ipKey, parseLimit, readCounter } from "./limits";
import { validateReport, type Report } from "./schema";
import { computeSignature } from "./signature";

const MAX_BODY_BYTES = 16 * 1024;
const HOUR_SECONDS = 3600;
const DAY_SECONDS = 86400;
const SIGNATURE_TTL_SECONDS = 90 * DAY_SECONDS;

interface SignatureRecord {
  issue: number;
  count: number;
  lastCommentDay: string;
  versionsToday: string[];
  platformsToday: string[];
}

function json(status: number, payload: Record<string, unknown>): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });
}

function addUnique(list: string[], value: string): string[] {
  return list.includes(value) ? list : [...list, value].slice(0, 20);
}

function platformLabel(report: Report): string {
  return `${report.platform} ${report.os_version} ${report.arch}`;
}

async function readBodyWithCap(request: Request): Promise<string | null> {
  const declared = Number(request.headers.get("content-length") ?? "0");
  if (declared > MAX_BODY_BYTES) {
    return null;
  }
  const reader = request.body?.getReader();
  if (!reader) {
    return "";
  }
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    total += value.byteLength;
    if (total > MAX_BODY_BYTES) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const merged = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder().decode(merged);
}

// Only the address is counted. The install id is chosen by the sender, so a per-install limit
// would stop nobody; put a Cloudflare rate-limiting rule in front of the Worker for more.
async function overRateLimit(env: Env, request: Request, now: Date): Promise<boolean> {
  const ip = request.headers.get("cf-connecting-ip") ?? "unknown";
  const ipCount = await bumpCounter(env.REPORTS, await ipKey(ip, now), HOUR_SECONDS);
  return ipCount > parseLimit(env.MAX_REPORTS_PER_IP_PER_HOUR, 10);
}

// Crashes and failed updates have their own daily budget, so noisy kinds cannot use it up.
function isHighPriority(report: Report): boolean {
  return report.kind === "crash" || report.kind === "panic" || report.kind === "update-failed";
}

function issueCapFor(env: Env, report: Report, today: string): { key: string; limit: number } {
  if (isHighPriority(report)) {
    return { key: `cap:issues:high:${today}`, limit: parseLimit(env.MAX_NEW_ISSUES_PER_DAY, 30) };
  }
  return { key: `cap:issues:low:${today}`, limit: parseLimit(env.MAX_NEW_LOW_PRIORITY_ISSUES_PER_DAY, 10) };
}

async function handleKnownSignature(
  env: Env,
  key: string,
  record: SignatureRecord,
  report: Report,
  now: Date,
): Promise<Response> {
  const today = dayStamp(now);
  const sameDay = record.lastCommentDay === today;
  const versions = addUnique(sameDay ? record.versionsToday : [], report.app_version);
  const platforms = addUnique(sameDay ? record.platformsToday : [], platformLabel(report));
  const updated: SignatureRecord = { ...record, count: record.count + 1, versionsToday: versions, platformsToday: platforms };
  if (!sameDay) {
    await addComment(env, record.issue, commentBody(versions, platforms, updated.count));
    updated.lastCommentDay = today;
  }
  await env.REPORTS.put(key, JSON.stringify(updated), { expirationTtl: SIGNATURE_TTL_SECONDS });
  return json(200, { status: "duplicate", issue: record.issue });
}

async function handleNewSignature(
  env: Env,
  key: string,
  signature: string,
  report: Report,
  now: Date,
): Promise<Response> {
  const today = dayStamp(now);
  const cap = issueCapFor(env, report, today);
  const issuesToday = await readCounter(env.REPORTS, cap.key);
  if (issuesToday >= cap.limit) {
    return json(429, { error: "daily issue cap reached" });
  }
  await bumpCounter(env.REPORTS, cap.key, DAY_SECONDS);
  const issue = await createIssue(env, issueTitle(report), issueBody(report, signature));
  const record: SignatureRecord = {
    issue,
    count: 1,
    lastCommentDay: today,
    versionsToday: [report.app_version],
    platformsToday: [platformLabel(report)],
  };
  await env.REPORTS.put(key, JSON.stringify(record), { expirationTtl: SIGNATURE_TTL_SECONDS });
  return json(201, { status: "created", issue });
}

async function handleReport(request: Request, env: Env): Promise<Response> {
  const contentType = (request.headers.get("content-type") ?? "").toLowerCase();
  if (!contentType.startsWith("application/json")) {
    return json(415, { error: "content-type must be application/json" });
  }
  const text = await readBodyWithCap(request);
  if (text === null) {
    return json(413, { error: "body too large" });
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return json(400, { error: "invalid json" });
  }
  const validation = validateReport(parsed);
  if (!validation.ok) {
    return json(400, { error: validation.reason });
  }
  const report = validation.report;
  const now = new Date();
  if (await overRateLimit(env, request, now)) {
    return json(429, { error: "rate limited" });
  }
  const signature = await computeSignature(report);
  const key = `sig:${signature}`;
  const stored = await env.REPORTS.get(key);
  if (stored) {
    return handleKnownSignature(env, key, JSON.parse(stored) as SignatureRecord, report, now);
  }
  return handleNewSignature(env, key, signature, report, now);
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    try {
      if (url.pathname === "/health") {
        return request.method === "GET" ? new Response("ok") : json(405, { error: "method not allowed" });
      }
      if (url.pathname === "/v1/report") {
        return request.method === "POST" ? await handleReport(request, env) : json(405, { error: "method not allowed" });
      }
      return json(404, { error: "not found" });
    } catch (error) {
      // Only the message from our own code is logged, never the request body.
      console.error(JSON.stringify({ event: "unhandled", message: error instanceof Error ? error.message : "unknown" }));
      return json(502, { error: "upstream failure" });
    }
  },
} satisfies ExportedHandler<Env>;
