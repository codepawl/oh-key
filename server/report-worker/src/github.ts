import { matchMessageTemplate, type Report } from "./schema";

const API_ROOT = "https://api.github.com";
const MAX_TITLE_LENGTH = 100;

// The title is built from the kind, the name of the message template and the checked source
// location, never from the sender's text.
export function issueTitle(report: Report): string {
  const template = matchMessageTemplate(report.kind, report.message);
  const place = report.source_location ? ` at ${report.source_location}` : "";
  const title = `[auto] ${report.kind}: ${template?.title ?? "report"}${place}`;
  return title.length > MAX_TITLE_LENGTH ? `${title.slice(0, MAX_TITLE_LENGTH - 1)}…` : title;
}

// A code span shows text as it is: no @mention, #reference, link, image or HTML is made from it.
function escapeCell(value: string): string {
  const flat = value.replace(/[\r\n]+/g, " ").replaceAll("`", "'").replace(/\|/g, "\\|");
  return `\`${flat}\``;
}

function fence(text: string): string {
  const safe = text.replaceAll("```", "'''");
  return "```\n" + safe + "\n```";
}

export function issueBody(report: Report, signature: string): string {
  const rows: Array<[string, string]> = [
    ["App", `${report.app} ${report.app_version}`],
    ["Platform", `${report.platform} ${report.os_version} (${report.arch})`],
    ["Kind", report.kind],
    ["Message", report.message],
    ["Source", report.source_location ?? "n/a"],
    ["Install", report.install_id.slice(0, 8)],
    ["Counts", report.counts ? JSON.stringify(report.counts) : "n/a"],
    ["Signature", signature],
  ];
  const tableRows = rows.map(([name, value]) => `| ${name} | ${escapeCell(value)} |`);
  const sections = [["| Field | Value |", "| --- | --- |", ...tableRows].join("\n")];
  if (report.backtrace?.length) {
    sections.push("### Backtrace\n" + fence(report.backtrace.join("\n")));
  }
  if (report.log_lines?.length) {
    sections.push("### Log lines\n" + fence(report.log_lines.join("\n")));
  }
  return sections.join("\n\n");
}

export function commentBody(versions: string[], platforms: string[], count: number): string {
  return [
    `Seen again (total ${count}).`,
    `Versions today: ${versions.map(escapeCell).join(", ")}`,
    `Platforms today: ${platforms.map(escapeCell).join(", ")}`,
  ].join("\n");
}

function headers(token: string): HeadersInit {
  return {
    authorization: `Bearer ${token}`,
    accept: "application/vnd.github+json",
    "content-type": "application/json",
    "user-agent": "oh-key-report-worker",
    "x-github-api-version": "2022-11-28",
  };
}

export async function createIssue(env: Env, title: string, body: string): Promise<number> {
  const response = await fetch(`${API_ROOT}/repos/${env.GITHUB_REPO}/issues`, {
    method: "POST",
    headers: headers(env.GITHUB_TOKEN),
    body: JSON.stringify({ title, body, labels: ["auto-report"] }),
  });
  if (!response.ok) {
    throw new Error(`github issue create failed: ${response.status}`);
  }
  const created = (await response.json()) as { number: number };
  return created.number;
}

export async function addComment(env: Env, issueNumber: number, body: string): Promise<void> {
  const response = await fetch(`${API_ROOT}/repos/${env.GITHUB_REPO}/issues/${issueNumber}/comments`, {
    method: "POST",
    headers: headers(env.GITHUB_TOKEN),
    body: JSON.stringify({ body }),
  });
  if (!response.ok) {
    throw new Error(`github comment failed: ${response.status}`);
  }
}
