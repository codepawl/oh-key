// Reads the latest public release of oh-key without the GitHub REST API, whose 60 calls an hour
// per address run out on shared Cloudflare addresses. The tag comes from where releases/latest
// redirects (the same way the app's updater finds it) and the file list from SHA256SUMS.txt,
// which every release carries.

export const PUBLIC_REPOSITORY = "codepawl/oh-key-releases";
const RELEASES_URL = `https://github.com/${PUBLIC_REPOSITORY}/releases`;
const TAG_PATTERN = /\/releases\/tag\/(v\d+\.\d+\.\d+)$/;

export async function fetchLatestRelease(fetchFunction = fetch) {
  const latestResponse = await fetchFunction(`${RELEASES_URL}/latest`, { redirect: "manual" });
  const location = latestResponse.headers.get("location") || "";
  const tagMatch = location.match(TAG_PATTERN);
  if (!tagMatch) {
    throw new Error(`releases/latest named no release (${latestResponse.status} ${location})`);
  }
  const tag = tagMatch[1];
  const sumsResponse = await fetchFunction(`${RELEASES_URL}/download/${tag}/SHA256SUMS.txt`);
  if (!sumsResponse.ok) {
    throw new Error(`SHA256SUMS.txt for ${tag} returned ${sumsResponse.status}`);
  }
  const fileNames = parseChecksumFileNames(await sumsResponse.text());
  return describeRelease(tag, fileNames);
}

// Lines look like "<sha256>  <name>" or "<sha256> *<name>".
export function parseChecksumFileNames(sumsText) {
  const fileNames = [];
  for (const line of sumsText.split(/\r?\n/)) {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 2 || !/^[0-9a-f]{64}$/i.test(parts[0])) {
      continue;
    }
    fileNames.push(parts[parts.length - 1].replace(/^\*/, ""));
  }
  return fileNames;
}

export function describeRelease(tag, fileNames) {
  const downloadBase = `${RELEASES_URL}/download/${tag}`;
  const hasFile = (name) => fileNames.includes(name);
  const fileUrl = (name) => (hasFile(name) ? `${downloadBase}/${name}` : null);
  return {
    tag,
    version: tag.replace(/^v/, ""),
    pageUrl: `${RELEASES_URL}/tag/${tag}`,
    windowsSetup: fileUrl("oh-key-setup.exe"),
    windowsZips: ["x64", "arm64", "x86"]
      .map((architecture) => ({ architecture, url: fileUrl(`oh-key-windows-${architecture}.zip`) }))
      .filter((zip) => zip.url),
    macosZip: fileUrl("oh-key-macos.zip"),
    sourceZip: fileUrl("oh-key-source.zip"),
    checksums: `${downloadBase}/SHA256SUMS.txt`,
  };
}

// Where /download/<platform> sends people: the installer when the release has one.
export function downloadTarget(release, platform) {
  if (platform === "windows") {
    return release.windowsSetup || release.windowsZips[0]?.url || release.pageUrl;
  }
  if (platform === "macos") {
    return release.macosZip || release.pageUrl;
  }
  return release.pageUrl;
}

// "windows", "macos", "linux" or "phone", from the request headers. Client hints first, since
// some browsers freeze the platform in the User-Agent string.
export function detectPlatform(userAgent = "", platformHint = "") {
  const hint = platformHint.replace(/"/g, "").toLowerCase();
  if (hint === "windows") return "windows";
  if (hint === "macos") return "macos";
  if (hint === "android" || hint === "ios") return "phone";
  if (/iPhone|iPad|iPod|Android/i.test(userAgent)) return "phone";
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "macos";
  if (/Linux|X11|CrOS/i.test(userAgent)) return "linux";
  return "windows";
}
