import { test } from "node:test";
import assert from "node:assert/strict";
import { describeRelease, detectPlatform, downloadTarget, fetchLatestRelease, parseChecksumFileNames } from "../src/release.js";
import { renderPage } from "../src/page.js";
import worker from "../src/worker.js";
import { APPLE_TOUCH_ICON_PNG, FAVICON_ICO, FAVICON_SVG } from "../src/brand-favicons.js";

const HASH = "a".repeat(64);
const LINUX_FILES = ["oh-key-linux-x86_64.tar.gz", "oh-key_0.2.0_amd64.deb"];
const SUMS_WITH_SETUP = [
  "oh-key-windows-arm64.exe", "oh-key-windows-arm64.zip", "oh-key-windows-x64.exe",
  "oh-key-windows-x64.zip", "oh-key-windows-x86.exe", "oh-key-windows-x86.zip",
  "oh-key-source.zip", "oh-key-macos.zip", "oh-key-setup.exe",
].map((name) => `${HASH}  ${name}`).join("\n");

function releaseWithLinux() {
  const sums = [SUMS_WITH_SETUP, ...LINUX_FILES.map((name) => `${HASH}  ${name}`)].join("\n");
  return describeRelease("v0.2.0", parseChecksumFileNames(sums));
}

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
  const release = await fetchLatestRelease(fakeFetch("https://github.com/anxuanng/oh-key/releases/tag/v0.2.0", SUMS_WITH_SETUP));
  assert.equal(release.version, "0.2.0");
  assert.equal(release.windowsSetup, "https://github.com/anxuanng/oh-key/releases/download/v0.2.0/oh-key-setup.exe");
  assert.deepEqual(release.windowsZips.map((zip) => zip.architecture), ["x64", "arm64", "x86"]);
  assert.ok(release.macosZip.endsWith("/v0.2.0/oh-key-macos.zip"));
});

test("a redirect to something other than a tag is an error", async () => {
  await assert.rejects(fetchLatestRelease(fakeFetch("https://github.com/anxuanng/oh-key/releases", "")));
});

test("checksum lines with a star and blank lines parse", () => {
  assert.deepEqual(parseChecksumFileNames(`${HASH} *oh-key-macos.zip\n\nnot a line\n`), ["oh-key-macos.zip"]);
});

test("Windows downloads fall back to the x64 zip when a release has no installer", () => {
  const release = describeRelease("v0.1.0", ["oh-key-windows-x64.zip", "oh-key-macos.zip"]);
  assert.equal(downloadTarget(release, "windows"), "https://github.com/anxuanng/oh-key/releases/download/v0.1.0/oh-key-windows-x64.zip");
  assert.equal(downloadTarget(release, "macos"), "https://github.com/anxuanng/oh-key/releases/download/v0.1.0/oh-key-macos.zip");
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

test("Linux files are matched by pattern and /download/linux prefers the deb", () => {
  const base = "https://github.com/anxuanng/oh-key/releases/download/v0.2.0";
  const release = describeRelease("v0.2.0", [...LINUX_FILES, "oh-key-macos.zip"]);
  assert.equal(release.linuxDeb.name, "oh-key_0.2.0_amd64.deb");
  assert.equal(release.linuxDeb.url, `${base}/oh-key_0.2.0_amd64.deb`);
  assert.equal(release.linuxTarball, `${base}/oh-key-linux-x86_64.tar.gz`);
  assert.equal(downloadTarget(release, "linux"), release.linuxDeb.url);
  const tarballOnly = describeRelease("v0.2.0", ["oh-key-linux-x86_64.tar.gz"]);
  assert.equal(tarballOnly.linuxDeb, null);
  assert.equal(downloadTarget(tarballOnly, "linux"), release.linuxTarball);
});

test("a release without Linux files sends /download/linux to the release page", () => {
  const release = describeRelease("v0.1.0", ["oh-key-windows-x64.zip", "oh-key_0.1.0_arm64.deb"]);
  assert.equal(release.linuxDeb, null);
  assert.equal(release.linuxTarball, null);
  assert.equal(downloadTarget(release, "linux"), release.pageUrl);
});

test("a Linux visitor gets the Linux download when the release has it", () => {
  const release = releaseWithLinux();
  const page = renderPage({ release, platform: "linux" });
  assert.match(page, /class="primary" href="\/download\/linux"/);
  assert.match(page, /oh-key_0\.2\.0_amd64\.deb \(Ubuntu, Debian\)<\/p>/);
  assert.match(page, /sudo apt install \.\/oh-key_0\.2\.0_amd64\.deb/);
  assert.match(page, /ibus restart/);
  assert.match(page, /data-tab="linux" aria-selected="true"/);
  assert.match(page, /thử nghiệm/);
  assert.doesNotMatch(page, /chưa có file cho Linux/);
  const windowsPage = renderPage({ release, platform: "windows" });
  assert.match(windowsPage, /class="other-platform" href="\/download\/linux"/);
});

test("a Linux visitor falls back to Windows when the release has no Linux files", () => {
  const release = describeRelease("v0.1.0", ["oh-key-windows-x64.zip", "oh-key-macos.zip"]);
  const page = renderPage({ release, platform: "linux" });
  assert.match(page, /class="primary" href="\/download\/windows"/);
  assert.match(page, /chưa có file cho Linux/);
  assert.doesNotMatch(page, /href="\/download\/linux"/);
});

test("the page still renders when GitHub is unreachable", () => {
  const page = renderPage({ release: null, platform: "linux" });
  assert.match(page, /class="primary" href="\/download\/windows"/);
  assert.match(page, /releases\/latest/);
  assert.doesNotMatch(page, /href="\/download\/linux"/);
});

test("the page links CodePawl kit favicons and omits middle dots", async () => {
  const page = renderPage({ release: null, platform: "windows" });
  assert.doesNotMatch(page, /\u00b7/);
  assert.ok(page.includes('href="/favicon.svg"'));
  assert.ok(page.includes('href="/favicon.ico"'));
  assert.ok(page.includes('href="/apple-touch-icon.png"'));
  assert.ok(page.includes('href="https://codepawl.com"'));

  const svgResponse = await worker.fetch(new Request("https://ohkey.codepawl.com/favicon.svg"), {}, {});
  assert.equal(svgResponse.status, 200);
  assert.equal(await svgResponse.text(), FAVICON_SVG);

  const icoResponse = await worker.fetch(new Request("https://ohkey.codepawl.com/favicon.ico"), {}, {});
  assert.equal(icoResponse.status, 200);
  assert.deepEqual(new Uint8Array(await icoResponse.arrayBuffer()), FAVICON_ICO);

  const touchResponse = await worker.fetch(new Request("https://ohkey.codepawl.com/apple-touch-icon.png"), {}, {});
  assert.equal(touchResponse.status, 200);
  assert.deepEqual(new Uint8Array(await touchResponse.arrayBuffer()), APPLE_TOUCH_ICON_PNG);
});
