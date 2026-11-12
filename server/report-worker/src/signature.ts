import type { Report } from "./schema";

const TOP_FRAME_COUNT = 5;

// Drops everything that varies between machines or runs, so one bug gets one signature.
export function normalizeMessage(message: string): string {
  return message
    .replace(/[A-Za-z]:[\\/][^\s"']*/g, "<path>")
    .replace(/(?:\/[\w.-]+){2,}/g, "<path>")
    .replace(/0x[0-9a-fA-F]+/g, "<n>")
    .replace(/\d+/g, "<n>")
    .replace(/\s+/g, " ")
    .trim()
    .toLowerCase();
}

export function normalizeFrame(frame: string): string {
  return frame.replace(/0x[0-9a-fA-F]+/g, "").replace(/\+\s*<?\d+>?/g, "").trim();
}

export function signatureInput(report: Report): string {
  const frames = (report.backtrace ?? []).slice(0, TOP_FRAME_COUNT).map(normalizeFrame);
  return [report.kind, normalizeMessage(report.message), ...frames].join("\n");
}

export async function computeSignature(report: Report): Promise<string> {
  const bytes = new TextEncoder().encode(signatureInput(report));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  const hex = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  return hex.slice(0, 32);
}
