// ohkey.codepawl.com: the oh-key download page. Every request reads the latest public release
// (cached for a few minutes), so a new version shows up here as soon as it is published.
//   /                    the page, leading with the visitor's OS
//   /download/windows    the Windows installer of the latest release
//   /download/macos      the macOS zip of the latest release
//   /download/linux      the Linux .deb (or tarball) of the latest release
//   /install.ps1         the PowerShell install script from the releases repo

import { detectPlatform, downloadTarget, fetchLatestRelease, PUBLIC_REPOSITORY } from "./release.js";
import { renderPage } from "./page.js";
import { APPLE_TOUCH_ICON_PNG, FAVICON_ICO, FAVICON_SVG } from "./brand-favicons.js";

const RELEASE_CACHE_KEY = "https://ohkey.codepawl.com/__cache/latest-release";
const RELEASE_CACHE_SECONDS = 300;
const INSTALL_SCRIPT_URL = `https://raw.githubusercontent.com/${PUBLIC_REPOSITORY}/main/install.ps1`;

export default {
  async fetch(request, environment, context) {
    const url = new URL(request.url);
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method not allowed", { status: 405 });
    }
    if (url.pathname === "/install.ps1") {
      return serveInstallScript();
    }
    if (url.pathname === "/favicon.svg") {
      return new Response(FAVICON_SVG, { headers: { "content-type": "image/svg+xml", "cache-control": "public, max-age=86400", "x-content-type-options": "nosniff" } });
    }
    if (url.pathname === "/favicon.ico") {
      return new Response(FAVICON_ICO, { headers: { "content-type": "image/x-icon", "cache-control": "public, max-age=86400", "x-content-type-options": "nosniff" } });
    }
    if (url.pathname === "/apple-touch-icon.png") {
      return new Response(APPLE_TOUCH_ICON_PNG, { headers: { "content-type": "image/png", "cache-control": "public, max-age=86400", "x-content-type-options": "nosniff" } });
    }
    const downloadMatch = url.pathname.match(/^\/download\/(windows|macos|linux)\/?$/);
    if (downloadMatch) {
      const release = await cachedLatestRelease(context);
      const fallback = `https://github.com/${PUBLIC_REPOSITORY}/releases/latest`;
      const target = release ? downloadTarget(release, downloadMatch[1]) : fallback;
      return Response.redirect(target, 302);
    }
    if (url.pathname !== "/") {
      return Response.redirect(`${url.origin}/`, 301);
    }
    const release = await cachedLatestRelease(context);
    const platform = detectPlatform(
      request.headers.get("user-agent") || "",
      request.headers.get("sec-ch-ua-platform") || "",
    );
    return new Response(renderPage({ release, platform }), {
      headers: {
        "content-type": "text/html; charset=utf-8",
        "cache-control": "private, max-age=60",
        "accept-ch": "Sec-CH-UA-Platform",
        vary: "User-Agent, Sec-CH-UA-Platform",
        "x-content-type-options": "nosniff",
        "referrer-policy": "strict-origin-when-cross-origin",
      },
    });
  },
};

// The release, or null when GitHub cannot be reached: the page then links to the releases list.
async function cachedLatestRelease(context) {
  const cache = caches.default;
  const cached = await cache.match(RELEASE_CACHE_KEY);
  if (cached) {
    return cached.json();
  }
  try {
    const release = await fetchLatestRelease();
    const response = new Response(JSON.stringify(release), {
      headers: {
        "content-type": "application/json",
        "cache-control": `max-age=${RELEASE_CACHE_SECONDS}`,
      },
    });
    context.waitUntil(cache.put(RELEASE_CACHE_KEY, response));
    return release;
  } catch (error) {
    console.error("latest release lookup failed", String(error));
    return null;
  }
}

async function serveInstallScript() {
  const upstream = await fetch(INSTALL_SCRIPT_URL, { cf: { cacheTtl: RELEASE_CACHE_SECONDS } });
  if (!upstream.ok) {
    return new Response("install.ps1 is not reachable right now", { status: 502 });
  }
  return new Response(upstream.body, {
    headers: {
      "content-type": "text/plain; charset=utf-8",
      "cache-control": `public, max-age=${RELEASE_CACHE_SECONDS}`,
    },
  });
}
