import { env } from "cloudflare:workers";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker from "../src/index";
import { normalizeMessage, signatureInput } from "../src/signature";
import { issueBody, issueTitle } from "../src/github";
import { validateReport, type Report } from "../src/schema";
import macosFixture from "../fixtures/macos-report.json";
import windowsFixture from "../fixtures/windows-report.json";

const INSTALL_ID = "123e4567-e89b-42d3-a456-426614174000";

function sampleReport(overrides: Partial<Report> = {}): Report {
  return {
    app: "oh-key",
    app_version: "0.2.0",
    platform: "windows",
    os_version: "10.0.26300",
    arch: "x86_64",
    install_id: INSTALL_ID,
    kind: "panic",
    message: "panic: index out of bounds",
    source_location: "crates/oh-key-engine/src/lib.rs:120:5",
    backtrace: ["oh_key_engine::apply", "oh_key_app::hook::on_key"],
    log_lines: ["1700000000 update check failed: no connection"],
    ...overrides,
  };
}

function post(body: unknown, ip = "203.0.113.1", raw?: string): Request {
  return new Request("https://worker.test/v1/report", {
    method: "POST",
    headers: { "content-type": "application/json", "cf-connecting-ip": ip },
    body: raw ?? JSON.stringify(body),
  });
}

function send(request: Request): Promise<Response> {
  return worker.fetch(request, env as unknown as Env);
}

let githubCalls: Array<{ url: string; body: Record<string, unknown> }>;

beforeEach(() => {
  githubCalls = [];
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = typeof input === "string" ? input : (input as Request).url;
    githubCalls.push({ url, body: JSON.parse(String(init?.body ?? "{}")) });
    const isIssueCreate = url.endsWith("/issues");
    return new Response(JSON.stringify(isIssueCreate ? { number: 42 } : { id: 1 }), { status: 201 });
  });
});

afterEach(async () => {
  vi.restoreAllMocks();
  const listing = await env.REPORTS.list();
  await Promise.all(listing.keys.map((key) => env.REPORTS.delete(key.name)));
});

describe("routes", () => {
  it("answers /health", async () => {
    const response = await send(new Request("https://worker.test/health"));
    expect(response.status).toBe(200);
    expect(await response.text()).toBe("ok");
  });

  it("rejects the wrong method", async () => {
    const response = await send(new Request("https://worker.test/v1/report"));
    expect(response.status).toBe(405);
  });

  it("rejects non-JSON content", async () => {
    const request = new Request("https://worker.test/v1/report", {
      method: "POST",
      headers: { "content-type": "text/plain" },
      body: "hello",
    });
    expect((await send(request)).status).toBe(415);
  });

  it("rejects invalid JSON", async () => {
    expect((await send(post(null, "203.0.113.1", "{nope"))).status).toBe(400);
  });
});

describe("validation", () => {
  it("rejects unknown fields", async () => {
    const response = await send(post({ ...sampleReport(), window_title: "secret" }));
    expect(response.status).toBe(400);
    expect(githubCalls).toHaveLength(0);
  });

  it("rejects a bad kind and a bad install id", async () => {
    expect((await send(post({ ...sampleReport(), kind: "keylog" }))).status).toBe(400);
    expect((await send(post({ ...sampleReport(), install_id: "abc" }))).status).toBe(400);
  });

  it("accepts what the Windows and macOS apps really send", () => {
    expect(validateReport(windowsFixture).ok).toBe(true);
    expect(validateReport(macosFixture).ok).toBe(true);
  });

  it("rejects text the app cannot produce", () => {
    const rejected = [
      { message: "please visit http://phish.example" },
      { message: "panic: xin chào" },
      { source_location: "etc/passwd:1" },
      { backtrace: ["@everyone"] },
      { log_lines: ["1700000000 hello @octocat"] },
      { log_lines: ["1700000000 crash in 0.1.0: café"] },
      { app_version: "1.0.0 <b>" },
      { os_version: "Windows <script>" },
      { arch: "pentium" },
      { app: "other" },
      { counts: { secret: 1 } },
    ];
    for (const override of rejected) {
      expect(validateReport({ ...sampleReport(), ...override }).ok, JSON.stringify(override)).toBe(false);
    }
  });

  it("rejects an upper-case install id", () => {
    const upper = INSTALL_ID.toUpperCase();
    expect(validateReport({ ...sampleReport(), install_id: upper }).ok).toBe(false);
  });

  it("rejects bodies over 16 KB without echoing them", async () => {
    const huge = sampleReport({ message: "x".repeat(20 * 1024) });
    const response = await send(post(huge));
    expect(response.status).toBe(413);
    expect(await response.text()).not.toContain("xxxx");
  });
});

describe("issue creation and dedupe", () => {
  it("creates an issue with the auto-report label", async () => {
    const response = await send(post(sampleReport()));
    expect(response.status).toBe(201);
    expect(githubCalls).toHaveLength(1);
    const call = githubCalls[0];
    expect(call.url).toBe("https://api.github.com/repos/codepawl/oh-key/issues");
    expect(call.body.labels).toEqual(["auto-report"]);
    expect(String(call.body.title)).toBe("[auto] panic: panic at crates/oh-key-engine/src/lib.rs:120:5");
    expect(String(call.body.body)).toContain("| Platform | `windows 10.0.26300 (x86_64)` |");
    expect(String(call.body.body)).toContain("update check failed: no connection");
  });

  it("counts a repeat without a second issue", async () => {
    await send(post(sampleReport()));
    const repeat = sampleReport({ install_id: "223e4567-e89b-42d3-a456-426614174000" });
    const response = await send(post(repeat));
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ status: "duplicate", issue: 42 });
    expect(githubCalls).toHaveLength(1);
  });

  it("comments at most once a day per signature", async () => {
    await send(post(sampleReport()));
    const key = (await env.REPORTS.list({ prefix: "sig:" })).keys[0].name;
    const record = JSON.parse((await env.REPORTS.get(key)) as string);
    record.lastCommentDay = "2000-01-01";
    await env.REPORTS.put(key, JSON.stringify(record));

    await send(post(sampleReport({ app_version: "0.3.0" })));
    await send(post(sampleReport({ app_version: "0.4.0" })));
    const comments = githubCalls.filter((call) => call.url.endsWith("/issues/42/comments"));
    expect(comments).toHaveLength(1);
    expect(String(comments[0].body.body)).toContain("0.3.0");
  });

  it("gives different bugs different issues", async () => {
    await send(post(sampleReport()));
    await send(post(sampleReport({ kind: "error", message: "SendInput took 1 of 2 events" })));
    expect(githubCalls.filter((call) => call.url.endsWith("/issues"))).toHaveLength(2);
  });

  it("normalizes numbers and paths in the signature", () => {
    expect(normalizeMessage("failed at C:\\Users\\bob\\x.dll code 12")).toBe("failed at <path> code <n>");
    expect(signatureInput(sampleReport())).toContain("oh_key_engine::apply");
  });
});

describe("limits", () => {
  it("rate limits one IP", async () => {
    const limit = Number(env.MAX_REPORTS_PER_IP_PER_HOUR);
    let lastStatus = 0;
    for (let index = 0; index <= limit; index += 1) {
      const id = `${String(index).padStart(8, "0")}-e89b-42d3-a456-426614174000`;
      lastStatus = (await send(post(sampleReport({ install_id: id }), "198.51.100.9"))).status;
    }
    expect(lastStatus).toBe(429);
  });

  it("caps new crash issues per day", async () => {
    const limit = Number(env.MAX_NEW_ISSUES_PER_DAY);
    let lastStatus = 0;
    for (let index = 0; index <= limit; index += 1) {
      const id = `${String(index).padStart(8, "0")}-e89b-42d3-a456-426614174000`;
      const report = sampleReport({
        install_id: id,
        kind: "update-failed",
        message: `update to 1.0.0 failed: ${"a".repeat(index + 1)}`,
      });
      lastStatus = (await send(post(report, `10.0.${index}.1`))).status;
    }
    expect(lastStatus).toBe(429);
    expect(githubCalls.filter((call) => call.url.endsWith("/issues"))).toHaveLength(limit);
  });

  it("keeps room for crashes when the low-priority issues are used up", async () => {
    const limit = Number(env.MAX_NEW_LOW_PRIORITY_ISSUES_PER_DAY);
    let lastStatus = 0;
    for (let index = 0; index <= limit; index += 1) {
      const report = sampleReport({
        kind: "error",
        message: "SendInput took 1 of 2 events",
        backtrace: [`oh_key_app::frame${"a".repeat(index + 1)}`],
      });
      lastStatus = (await send(post(report, `10.1.${index}.1`))).status;
    }
    expect(lastStatus).toBe(429);
    const crash = sampleReport({ kind: "crash", message: "signal SIGSEGV", backtrace: undefined, source_location: undefined });
    expect((await send(post(crash, "10.2.0.1"))).status).toBe(201);
  });
});

describe("issue text", () => {
  it("shows sender text as code, so it pings and links nobody", () => {
    const hostile = sampleReport({ os_version: "@octocat [x](http://phish.example) #1 <img src=x>", arch: "a|b" });
    const body = issueBody(hostile, "sig");
    expect(body).toContain("`windows @octocat [x](http://phish.example) #1 <img src=x> (a\\|b)`");
  });

  it("builds the title from the template, not from the message", () => {
    const title = issueTitle(sampleReport({ source_location: undefined }));
    expect(title).toBe("[auto] panic: panic");
  });
});
