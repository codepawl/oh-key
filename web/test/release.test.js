import { test } from "node:test";
import assert from "node:assert/strict";
import { describeRelease, detectPlatform, downloadTarget, fetchLatestRelease, parseChecksumFileNames } from "../src/release.js";
import { renderPage } from "../src/page.js";

const HASH = "a".repeat(64);
const SUMS_WITH_SETUP = [
  "oh-key-windows-arm64.exe", "oh-key-windows-arm64.zip", "oh-key-windows-x64.exe",
  "oh-key-windows-x64.zip", "oh-key-windows-x86.exe", "oh-key-windows-x86.zip",
  "oh-key-source.zip", "oh-key-macos.zip", "oh-key-setup.exe",
].map((name) => `${HASH}  ${name}`).join("\n");

function fakeFetch(location, sumsText) {
  return async (url) => {
    if (url.endsWith("/releases/latest")) {
      return new Response(null, { status: 302, headers: { location } });
    }
    if (url.endsWith("/SHA256SUMS.txt")) {
      return new Response(sumsText, { status: 200 });
    }
    return new Response("not found", { status: 404 });
  };
}

test("reads the tag from the redirect and the files from SHA256SUMS.txt", async () => {
  const release = await fetchLatestRelease(fakeFetch("https://github.com/codepawl/oh-key-releases/releases/tag/v0.2.0", SUMS_WITH_SETUP));
  assert.equal(release.version, "0.2.0");
  assert.equal(release.windowsSetup, "https://github.com/codepawl/oh-key-releases/releases/download/v0.2.0/oh-key-setup.exe");
  assert.deepEqual(release.windowsZips.map((zip) => zip.architecture), ["x64", "arm64", "x86"]);
  assert.ok(release.macosZip.endsWith("/v0.2.0/oh-key-macos.zip"));
});

test("a redirect to something other than a tag is an error", async () => {
  await assert.rejects(fetchLatestRelease(fakeFetch("https://github.com/codepawl/oh-key-releases/releases", "")));
});

test("checksum lines with a star and blank lines parse", () => {
  assert.deepEqual(parseChecksumFileNames(`${HASH} *oh-key-macos.zip\n\nnot a line\n`), ["oh-key-macos.zip"]);
});

test("Windows downloads fall back to the x64 zip when a release has no installer", () => {
  const release = describeRelease("v0.1.0", ["oh-key-windows-x64.zip", "oh-key-macos.zip"]);
  assert.equal(downloadTarget(release, "windows"), "https://github.com/codepawl/oh-key-releases/releases/download/v0.1.0/oh-key-windows-x64.zip");
  assert.equal(downloadTarget(release, "macos"), "https://github.com/codepawl/oh-key-releases/releases/download/v0.1.0/oh-key-macos.zip");
});

test("platform detection", () => {
  assert.equal(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), "windows");
  assert.equal(detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"), "macos");
  assert.equal(detectPlatform("Mozilla/5.0 (X11; Linux x86_64)"), "linux");
  assert.equal(detectPlatform("Mozilla/5.0 (Linux; Android 14; Pixel 8)"), "phone");
  assert.equal(detectPlatform("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)"), "phone");
  assert.equal(detectPlatform("Mozilla/5.0 (Windows NT 10.0)", '"macOS"'), "macos");
});

test("the page leads with the visitor's platform and lists every file", () => {
  const release = describeRelease("v0.2.0", parseChecksumFileNames(SUMS_WITH_SETUP));
  const windowsPage = renderPage({ release, platform: "windows" });
  assert.match(windowsPage, /class="primary" href="\/download\/windows"/);
  assert.match(windowsPage, /oh-key-setup\.exe/);
  assert.match(windowsPage, /oh-key-windows-arm64\.zip/);
  assert.match(windowsPage, /Bản 0\.2\.0/);
  const macPage = renderPage({ release, platform: "macos" });
  assert.match(macPage, /class="primary" href="\/download\/macos"/);
  for (const url of ["https://x.com/anxuanng", "https://github.com/codepawl", "https://www.threads.com/@codepawl", "https://www.youtube.com/@codepawlhq"]) {
    assert.ok(windowsPage.includes(url), url);
  }
});

test("the page still renders when GitHub is unreachable", () => {
  const page = renderPage({ release: null, platform: "linux" });
  assert.match(page, /Chưa có bản cho Linux/);
  assert.match(page, /releases\/latest/);
});
