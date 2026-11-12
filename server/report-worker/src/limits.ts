// KV counters. Not atomic, which is fine: the limits are soft abuse guards, not billing.
// Keys expire on their own, so the namespace never needs cleaning.

export async function bumpCounter(kv: KVNamespace, key: string, ttlSeconds: number): Promise<number> {
  const current = Number((await kv.get(key)) ?? "0");
  const next = current + 1;
  await kv.put(key, String(next), { expirationTtl: ttlSeconds });
  return next;
}

export async function readCounter(kv: KVNamespace, key: string): Promise<number> {
  return Number((await kv.get(key)) ?? "0");
}

export function dayStamp(now: Date): string {
  return now.toISOString().slice(0, 10);
}

export function hourStamp(now: Date): string {
  return now.toISOString().slice(0, 13);
}

export function parseLimit(raw: string | undefined, fallback: number): number {
  const parsed = Number(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

async function sha256Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

// IPs are hashed so the namespace never stores one.
export async function ipKey(ip: string, now: Date): Promise<string> {
  return `rl:ip:${hourStamp(now)}:${(await sha256Hex(ip)).slice(0, 24)}`;
}
