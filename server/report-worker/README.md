# oh-key-report

Cloudflare Worker that receives crash and error reports from oh-key and files them as GitHub issues in the private `codepawl/oh-key` repo, labelled `auto-report`.

- `POST /v1/report`: JSON body up to 16 KB, strict schema (unknown fields are rejected). Returns 201 (new issue), 200 (duplicate counted), 400, 413, 415, 429.
- `GET /health`: returns `ok`.

Privacy: the schema has no field for typed text, window titles or the foreground exe name (only an optional hash). The Worker never logs or echoes a request body, and stores IPs only as a hash in an hourly counter.

## How reports are grouped

Signature = SHA-256 of kind, the message with numbers and paths removed, and the top 5 backtrace frames. KV maps `sig:<hash>` to the issue number and a count. A repeat only bumps the count and adds at most one comment per signature per day, listing the versions and platforms seen that day.

## Limits (variables in wrangler.toml)

- `MAX_REPORTS_PER_IP_PER_HOUR` (10), `MAX_NEW_ISSUES_PER_DAY` (30, for crash, panic and update-failed), `MAX_NEW_LOW_PRIORITY_ISSUES_PER_DAY` (10, for the other kinds). The sender picks its install id, so there is no per-install limit.
- Only reports the app can produce are accepted (message templates, `oh_key_*` frames, `crates/*.rs` places, known log line openings). Add a Cloudflare rate-limiting rule in front of the Worker, and use a paid KV plan if the free 1,000 writes a day could run out.
- Counters live in KV, so each report costs a few KV writes. The free plan allows 1,000 KV writes a day; move to the paid plan or lower the limits if reports grow.

## Deploy (owner steps)

```
npm install
npx wrangler kv namespace create REPORTS
```

Put the printed id into `wrangler.toml` (`[[kv_namespaces]]`, replacing `REPLACE_WITH_KV_NAMESPACE_ID`). Create a fine-grained GitHub token with only `Issues: read and write` on `codepawl/oh-key`, then:

```
npx wrangler secret put GITHUB_TOKEN
npm run deploy
```

Make sure the `auto-report` label exists in the repo (GitHub drops unknown labels on issue creation only if you lack triage rights; creating the label first is safest). Then set `REPORT_ENDPOINT` in the Windows and macOS apps to `https://oh-key-report.<your-subdomain>.workers.dev/v1/report`.

## Develop

```
npm test        # vitest in the Workers runtime, GitHub API mocked
npm run typecheck
```
