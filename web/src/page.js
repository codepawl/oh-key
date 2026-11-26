// The download page as one HTML string. Everything shown comes from the release object, so a new
// release needs no edit here.

import { icons } from "./icons.js";

// Real engine output for each key of "Hoom nay minhf check email nhes" (oh-key-cli type <prefix>):
// "check" and "email" stay English, "oo" becomes "ô", "f" and "s" add the marks.
const DEMO_KEYS = "Hoom nay minhf check email nhes";
const DEMO_FRAMES = [
  "H", "Ho", "Hô", "Hôm", "Hôm ", "Hôm n", "Hôm na", "Hôm nay", "Hôm nay ", "Hôm nay m",
  "Hôm nay mi", "Hôm nay min", "Hôm nay minh", "Hôm nay mình", "Hôm nay mình ",
  "Hôm nay mình c", "Hôm nay mình ch", "Hôm nay mình che", "Hôm nay mình chec",
  "Hôm nay mình check", "Hôm nay mình check ", "Hôm nay mình check e", "Hôm nay mình check em",
  "Hôm nay mình check ema", "Hôm nay mình check emai", "Hôm nay mình check email",
  "Hôm nay mình check email ", "Hôm nay mình check email n", "Hôm nay mình check email nh",
  "Hôm nay mình check email nhe", "Hôm nay mình check email nhé",
];

const SOCIAL_LINKS = [
  { name: "X", handle: "@anxuanng", url: "https://x.com/anxuanng", icon: icons.x },
  { name: "GitHub", handle: "codepawl", url: "https://github.com/codepawl", icon: icons.github },
  { name: "Threads", handle: "@codepawl", url: "https://www.threads.com/@codepawl", icon: icons.threads },
  { name: "YouTube", handle: "@codepawlhq", url: "https://www.youtube.com/@codepawlhq", icon: icons.youtube },
];

const MAC_QUARANTINE_COMMAND = 'xattr -dr com.apple.quarantine "$HOME/Library/Input Methods/oh-key.app"';
const POWERSHELL_COMMAND = "irm https://ohkey.codepawl.com/install.ps1 | iex";
const LINUX_TARBALL_NAME = "oh-key-linux-x86_64.tar.gz";
const LINUX_TARBALL_COMMAND = `tar xzf ${LINUX_TARBALL_NAME} && cd oh-key-linux-x86_64 && sudo ./install.sh`;
const LINUX_BETA_NOTE = "Bản Linux đang thử nghiệm: đã build và chạy test trong CI, chưa được thử trên máy thật.";

// Releases from before the Linux build, and cached release objects, carry no Linux files.
function hasLinuxFiles(release) {
  return Boolean(release?.linuxDeb || release?.linuxTarball);
}

// The platform whose download leads the page: a Linux visitor gets Windows until a release has Linux files.
function leadingPlatform(platform, release) {
  return platform === "linux" && !hasLinuxFiles(release) ? "windows" : platform;
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function codeLine(command) {
  return `<div class="code"><code>${escapeHtml(command)}</code><button class="copy" type="button" data-copy="${escapeHtml(command)}" aria-label="Chép lệnh">${icons.copy}${icons.check}</button></div>`;
}

function primaryButton(platform, release) {
  if (platform === "macos") {
    return `<a class="primary" href="/download/macos">${icons.apple}<span>Tải cho macOS</span></a>
      <p class="file-note">oh-key-macos.zip, Apple Silicon và Intel, macOS 12 trở lên</p>
      <p class="status status-beta">${icons.flask}<span>Bản macOS đang thử nghiệm, chưa được thử kỹ trong ứng dụng thật.</span></p>`;
  }
  if (platform === "linux") {
    const fileName = release.linuxDeb ? release.linuxDeb.name : LINUX_TARBALL_NAME;
    const distributions = release.linuxDeb ? "Ubuntu, Debian" : "IBus";
    return `<a class="primary" href="/download/linux">${icons.linux}<span>Tải cho Linux</span></a>
      <p class="file-note">${escapeHtml(fileName)} (${distributions})</p>
      <p class="status status-beta">${icons.flask}<span>${LINUX_BETA_NOTE}</span></p>`;
  }
  const fileName = release?.windowsSetup ? "oh-key-setup.exe" : "oh-key-windows-x64.zip";
  return `<a class="primary" href="/download/windows">${icons.windows}<span>Tải cho Windows</span></a>
    <p class="file-note">${fileName}, Windows 10 và 11, x64, ARM64, x86</p>`;
}

const PLATFORM_LINKS = [
  { id: "windows", icon: icons.windows },
  { id: "macos", icon: icons.apple },
  { id: "linux", icon: icons.linux },
];
const PLATFORM_LABELS = { windows: "Windows", macos: "macOS", linux: "Linux" };

function otherPlatformLinks(leading, release) {
  return PLATFORM_LINKS
    .filter((link) => link.id !== leading && (link.id !== "linux" || hasLinuxFiles(release)))
    .map((link) => `<a class="other-platform" href="/download/${link.id}">${link.icon}<span>Tải cho ${PLATFORM_LABELS[link.id]}</span></a>`)
    .join("");
}

function platformNotice(platform, release) {
  if (platform === "linux" && !hasLinuxFiles(release)) {
    return `<p class="status status-off">${icons.ban}<span>Bản này chưa có file cho Linux. Bản Windows và macOS ở dưới.</span></p>`;
  }
  if (platform === "phone") {
    return `<p class="status status-off">${icons.monitor}<span>oh-key chạy trên máy tính. Mở trang này trên Windows hoặc Mac để tải.</span></p>`;
  }
  return "";
}

function windowsSteps(release) {
  if (release?.windowsSetup) {
    return `<ol class="steps">
      <li><p>Tải <strong class="nowrap">oh-key-setup.exe</strong> rồi mở nó. Một file cài đúng bản cho máy x64, ARM64 hay x86.</p></li>
      <li><p>Nếu Windows hiện màn hình xanh SmartScreen, bấm <strong>Thông tin thêm</strong> rồi <strong>Vẫn chạy</strong>. oh-key chưa có chữ ký số nên Windows chưa nhận ra nó.</p></li>
      <li><p>Đồng ý giấy phép GPL, bấm <strong>Next</strong>, rồi <strong>Finish</strong>. Không cần quyền quản trị.</p></li>
      <li><p>Chữ <strong>V</strong> ở khay hệ thống là đang gõ tiếng Việt. Bấm vào nó hoặc nhấn <kbd>Ctrl</kbd> + <kbd>Shift</kbd> để đổi sang <strong>E</strong>, chuột phải để mở bảng cài đặt.</p></li>
    </ol>
    <div class="alternative">
      <p>Hoặc cài bằng PowerShell, không cần tải gì trước:</p>
      ${codeLine(POWERSHELL_COMMAND)}
    </div>`;
  }
  return `<ol class="steps">
      <li><p>Mở <strong>PowerShell</strong> từ menu Start, dán lệnh này và nhấn Enter:</p>${codeLine(POWERSHELL_COMMAND)}</li>
      <li><p>Lệnh tải bản mới nhất đúng cho máy, kiểm tra SHA-256, cài vào thư mục người dùng và chạy luôn. Không cần quyền quản trị.</p></li>
      <li><p>Chữ <strong>V</strong> ở khay hệ thống là đang gõ tiếng Việt. Bấm vào nó hoặc nhấn <kbd>Ctrl</kbd> + <kbd>Shift</kbd> để đổi sang <strong>E</strong>, chuột phải để mở bảng cài đặt.</p></li>
    </ol>
    <div class="alternative"><p>Hoặc tải file zip ở phần tất cả các bản, giải nén và chạy <strong class="nowrap">oh-key.exe</strong>.</p></div>`;
}

function macosSteps() {
  return `<p class="status status-beta">${icons.flask}<span>Bản macOS đang thử nghiệm, chưa được thử kỹ trong ứng dụng thật.</span></p>
    <ol class="steps">
      <li><p>Tải <strong class="nowrap">oh-key-macos.zip</strong> và mở nó để giải nén.</p></li>
      <li><p>Trong Finder, nhấn <kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>G</kbd>, gõ <strong>~/Library/Input Methods</strong> và kéo <strong class="nowrap">oh-key.app</strong> vào thư mục đó.</p></li>
      <li><p>Mở <strong>Terminal</strong> và chạy lệnh này. Bản build chưa notarize nên Gatekeeper sẽ chặn nếu bỏ bước này.</p>${codeLine(MAC_QUARANTINE_COMMAND)}</li>
      <li><p>Đăng xuất rồi đăng nhập lại.</p></li>
      <li><p>Vào <strong>System Settings → Keyboard → Input Sources</strong>, bấm <strong>+</strong>, chọn <strong>Vietnamese</strong> rồi <strong>oh-key</strong>. Nhấn <kbd>Ctrl</kbd> + <kbd>Shift</kbd> hoặc <kbd>Option</kbd> + <kbd>Z</kbd> để đổi V và E.</p></li>
    </ol>`;
}

function linuxSteps(release) {
  if (!hasLinuxFiles(release)) {
    return `<p class="status status-off">${icons.ban}<span>Bản này chưa có file cho Linux. Bản mới hơn sẽ có.</span></p>`;
  }
  const version = release.version;
  const installStep = release.linuxDeb
    ? `<li><p>Tải <strong class="nowrap">${escapeHtml(release.linuxDeb.name)}</strong> rồi cài bằng lệnh này (Ubuntu, Debian và các bản dẫn xuất):</p>${codeLine(`sudo apt install ./${release.linuxDeb.name}`)}</li>`
    : `<li><p>Tải <strong class="nowrap">${LINUX_TARBALL_NAME}</strong>, giải nén rồi chạy bộ cài:</p>${codeLine(LINUX_TARBALL_COMMAND)}</li>`;
  const tarballAlternative = release.linuxDeb && release.linuxTarball
    ? `<div class="alternative">
      <p>Không dùng apt? Tải <strong class="nowrap">${LINUX_TARBALL_NAME}</strong> (${escapeHtml(version)}) và chạy:</p>
      ${codeLine(LINUX_TARBALL_COMMAND)}
    </div>`
    : "";
  return `<p class="status status-beta">${icons.flask}<span>${LINUX_BETA_NOTE}</span></p>
    <ol class="steps">
      ${installStep}
      <li><p>Khởi động lại IBus:</p>${codeLine("ibus restart")}</li>
      <li><p>Mở <strong>Settings → Keyboard → Input Sources</strong>, bấm <strong>+</strong> rồi thêm <strong>Vietnamese (oh-key)</strong>. Trên KDE và các môi trường khác, thêm nó trong <strong class="nowrap">ibus-setup</strong>.</p></li>
      <li><p>Nhấn <kbd>Ctrl</kbd> + <kbd>Shift</kbd> hoặc <kbd>Alt</kbd> + <kbd>Z</kbd> để đổi V và E.</p></li>
    </ol>
    ${tarballAlternative}`;
}

function fileRow(name, detail, url) {
  return `<li><a href="${escapeHtml(url)}">${icons.download}<span class="file-name">${escapeHtml(name)}</span></a><span class="file-detail">${escapeHtml(detail)}</span></li>`;
}

const ARCHITECTURE_DETAILS = {
  x64: "Máy Intel và AMD 64-bit",
  arm64: "Máy ARM như Snapdragon",
  x86: "Máy 32-bit đời cũ",
};

function allFiles(release) {
  if (!release) {
    return `<p class="status status-warn">${icons.warn}<span>Chưa đọc được danh sách bản từ GitHub. <a href="https://github.com/codepawl/oh-key-releases/releases/latest">Mở trang release</a>.</span></p>`;
  }
  const windowsRows = [];
  if (release.windowsSetup) {
    windowsRows.push(fileRow("oh-key-setup.exe", "Bộ cài, tự chọn đúng bản", release.windowsSetup));
  }
  for (const zip of release.windowsZips) {
    windowsRows.push(fileRow(`oh-key-windows-${zip.architecture}.zip`, ARCHITECTURE_DETAILS[zip.architecture], zip.url));
  }
  const macRows = release.macosZip ? [fileRow("oh-key-macos.zip", "Apple Silicon và Intel", release.macosZip)] : [];
  const linuxRows = [];
  if (release.linuxDeb) {
    linuxRows.push(fileRow(release.linuxDeb.name, "Ubuntu, Debian (x86_64)", release.linuxDeb.url));
  }
  if (release.linuxTarball) {
    linuxRows.push(fileRow(LINUX_TARBALL_NAME, "Mọi bản Linux có IBus (x86_64)", release.linuxTarball));
  }
  const otherRows = [];
  if (release.sourceZip) {
    otherRows.push(fileRow("oh-key-source.zip", "Mã nguồn đúng bản này, GPL-3.0", release.sourceZip));
  }
  otherRows.push(fileRow("SHA256SUMS.txt", "Mã kiểm tra của mọi file", release.checksums));
  return `<div class="file-groups">
      <div class="file-group"><h3>${icons.windows}Windows</h3><ul>${windowsRows.join("")}</ul></div>
      ${macRows.length ? `<div class="file-group"><h3>${icons.apple}macOS</h3><ul>${macRows.join("")}</ul></div>` : ""}
      ${linuxRows.length ? `<div class="file-group"><h3>${icons.linux}Linux</h3><ul>${linuxRows.join("")}</ul></div>` : ""}
      <div class="file-group"><h3>${icons.archive}Khác</h3><ul>${otherRows.join("")}</ul></div>
    </div>
    ${linuxRows.length ? "" : `<p class="status status-off">${icons.ban}<span>Bản này chưa có file cho Linux.</span></p>`}`;
}

export function renderPage({ release, platform }) {
  const leading = leadingPlatform(platform, release);
  const installTab = leading === "macos" || leading === "linux" ? leading : "windows";
  const versionLine = release
    ? `<a class="version" href="${escapeHtml(release.pageUrl)}">Bản ${escapeHtml(release.version)}</a>`
    : "";
  return `<!doctype html>
<html lang="vi">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>oh-key - Bộ gõ tiếng Việt</title>
<meta name="description" content="oh-key: bộ gõ tiếng Việt viết bằng Rust, nhanh, nhẹ, ít lỗi hơn Unikey. Tải miễn phí cho Windows, macOS và Linux.">
<meta property="og:title" content="oh-key - Bộ gõ tiếng Việt">
<meta property="og:description" content="Bộ gõ tiếng Việt nhanh, nhẹ, ít lỗi. Tải cho Windows, macOS và Linux.">
<meta property="og:url" content="https://ohkey.codepawl.com/">
<meta name="theme-color" content="#1f1e1d">
<link rel="icon" type="image/svg+xml" href="/favicon.svg">
<link rel="icon" href="/favicon.ico" sizes="any">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600&family=JetBrains+Mono:wght@400;600&display=swap" rel="stylesheet">
<style>${STYLES}</style>
</head>
<body>
<header class="top">
  <a class="brand" href="/">${icons.mark}<span>oh-key</span></a>
  <a class="top-link" href="https://github.com/codepawl" aria-label="CodePawl trên GitHub">${icons.github}</a>
</header>
<main>
  <section class="hero">
    <div class="hero-copy">
      <h1>Bộ gõ tiếng Việt nhanh, nhẹ, ít lỗi.</h1>
      <p class="lead">Gõ Telex hay VNI trong mọi ứng dụng. Từ tiếng Anh như <span class="mono">check</span> hay <span class="mono">email</span> vẫn giữ nguyên. Viết bằng Rust, miễn phí, mã nguồn mở.</p>
      ${platformNotice(platform, release)}
      <div class="download">
        ${primaryButton(leading, release)}
      </div>
      <div class="meta">${otherPlatformLinks(leading, release)}${versionLine}</div>
    </div>
    <figure class="demo" aria-label="Gõ Telex thành tiếng Việt">
      <div class="field"><span class="typed" data-final="${escapeHtml(DEMO_FRAMES[DEMO_FRAMES.length - 1])}">${escapeHtml(DEMO_FRAMES[DEMO_FRAMES.length - 1])}</span><span class="caret"></span></div>
      <figcaption class="keys">${icons.keyboard}<span class="keys-typed">${escapeHtml(DEMO_KEYS)}</span></figcaption>
    </figure>
  </section>

  <section class="install" id="cai-dat">
    <div class="section-head">
      <h2>Cài đặt</h2>
      <div class="tabs" role="tablist">
        <button type="button" role="tab" data-tab="windows" aria-selected="${installTab === "windows"}">${icons.windows}Windows</button>
        <button type="button" role="tab" data-tab="macos" aria-selected="${installTab === "macos"}">${icons.apple}macOS</button>
        <button type="button" role="tab" data-tab="linux" aria-selected="${installTab === "linux"}">${icons.linux}Linux</button>
      </div>
    </div>
    <div class="tab-panel" data-panel="windows" ${installTab === "windows" ? "" : "hidden"}>${windowsSteps(release)}</div>
    <div class="tab-panel" data-panel="macos" ${installTab === "macos" ? "" : "hidden"}>${macosSteps()}</div>
    <div class="tab-panel" data-panel="linux" ${installTab === "linux" ? "" : "hidden"}>${linuxSteps(release)}</div>
  </section>

  <section class="files" id="tat-ca">
    <div class="section-head"><h2>Tất cả các bản</h2>${versionLine}</div>
    ${allFiles(release)}
  </section>
</main>
<footer class="bottom">
  <nav class="social" aria-label="CodePawl">
    ${SOCIAL_LINKS.map((link) => `<a href="${link.url}" aria-label="${link.name} ${link.handle}">${link.icon}<span>${link.handle}</span></a>`).join("")}
  </nav>
  <p class="legal">oh-key là phần mềm tự do theo GPL-3.0. Làm bởi <a href="https://codepawl.com">CodePawl</a>.</p>
</footer>
<script>${clientScript()}</script>
</body>
</html>`;
}

function clientScript() {
  return `
(() => {
  const frames = ${JSON.stringify(DEMO_FRAMES)};
  const keys = ${JSON.stringify(DEMO_KEYS)};
  const typed = document.querySelector(".typed");
  const keysTyped = document.querySelector(".keys-typed");
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (typed && !reduceMotion) {
    let index = 0;
    const step = () => {
      if (index < frames.length) {
        typed.textContent = frames[index];
        keysTyped.textContent = keys.slice(0, index + 1);
        index += 1;
        setTimeout(step, keys[index - 1] === " " ? 220 : 110);
      } else {
        index = 0;
        setTimeout(step, 2600);
      }
    };
    typed.textContent = "";
    keysTyped.textContent = "";
    setTimeout(step, 500);
  }
  const tabs = document.querySelectorAll("[data-tab]");
  for (const tab of tabs) {
    tab.addEventListener("click", () => {
      for (const other of tabs) other.setAttribute("aria-selected", String(other === tab));
      for (const panel of document.querySelectorAll("[data-panel]")) {
        panel.hidden = panel.dataset.panel !== tab.dataset.tab;
      }
    });
  }
  for (const button of document.querySelectorAll("[data-copy]")) {
    button.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(button.dataset.copy);
        button.classList.add("copied");
        setTimeout(() => button.classList.remove("copied"), 1600);
      } catch (error) {
        button.classList.remove("copied");
      }
    });
  }
})();`;
}

const STYLES = `
:root {
  --base: #f6f4ee;
  --surface: #ecE9e0;
  --surface-strong: #e2ded3;
  --text: #1f1e1d;
  --text-dim: #5f5a51;
  --accent: #d97757;
  --accent-text: #a8482a;
  --on-accent: #1f1e1d;
  --beta: #8a5a00;
  --warn: #a23b1e;
  --windows: #0078d4;
  --apple: currentColor;
  color-scheme: light;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --base: #1f1e1d;
    --surface: #282725;
    --surface-strong: #31302d;
    --text: #f0eee6;
    --text-dim: #a39e93;
    --accent: #d97757;
    --accent-text: #e58e70;
    --on-accent: #1f1e1d;
    --beta: #e5b25c;
    --warn: #ef8a6b;
    --windows: #3a9bf0;
    color-scheme: dark;
  }
}
:root[data-theme="dark"] {
  --base: #1f1e1d;
  --surface: #282725;
  --surface-strong: #31302d;
  --text: #f0eee6;
  --text-dim: #a39e93;
  --accent: #d97757;
  --accent-text: #e58e70;
  --on-accent: #1f1e1d;
  --beta: #e5b25c;
  --warn: #ef8a6b;
  --windows: #3a9bf0;
  color-scheme: dark;
}
* { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; }
body {
  margin: 0;
  background: var(--base);
  color: var(--text);
  font: 400 16px/1.6 "Be Vietnam Pro", system-ui, sans-serif;
}
a { color: inherit; }
svg { flex: none; }
.mono, code, kbd, .brand, .typed, .keys, .file-name { font-family: "JetBrains Mono", ui-monospace, monospace; }
.top, main, .bottom { width: 100%; max-width: 1040px; margin: 0 auto; padding: 0 16px; }
@media (min-width: 720px) { .top, main, .bottom { padding: 0 32px; } }

.top { display: flex; align-items: center; justify-content: space-between; height: 72px; user-select: none; }
.brand { display: inline-flex; align-items: center; gap: 10px; font-weight: 600; font-size: 18px; text-decoration: none; }
.brand svg { width: 28px; height: 28px; }
.top-link { display: inline-flex; padding: 8px; border-radius: 8px; color: var(--text-dim); }
.top-link:hover { color: var(--text); background: var(--surface); }
.top-link svg { width: 22px; height: 22px; }

.hero { display: grid; gap: 40px; padding: 32px 0 72px; align-items: center; }
@media (min-width: 880px) { .hero { grid-template-columns: 1.05fr 1fr; gap: 56px; padding: 56px 0 104px; } }
h1 { font-size: clamp(34px, 6vw, 52px); line-height: 1.12; letter-spacing: -0.02em; font-weight: 600; margin: 0 0 18px; text-wrap: balance; }
.lead { font-size: 18px; color: var(--text-dim); margin: 0 0 28px; max-width: 34em; text-wrap: pretty; }
.lead .mono { color: var(--text); font-size: 0.92em; }

.download { display: grid; gap: 10px; justify-items: start; }
.primary {
  display: inline-flex; align-items: center; gap: 12px;
  background: var(--accent); color: var(--on-accent);
  font-weight: 600; font-size: 18px; text-decoration: none;
  padding: 16px 26px; border-radius: 12px; user-select: none;
  transition: transform 160ms ease-out, filter 160ms ease-out;
}
.primary:hover { filter: brightness(1.06); }
.primary:active { transform: scale(0.98); }
.primary svg { width: 22px; height: 22px; }
.primary .icon-windows rect { fill: var(--on-accent); }
.file-note { margin: 0; font-size: 14px; color: var(--text-dim); }
.meta { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 24px; margin-top: 22px; font-size: 15px; }
.other-platform { display: inline-flex; align-items: center; gap: 8px; text-decoration: none; font-weight: 500; padding: 6px 0; }
.other-platform:hover span { text-decoration: underline; text-underline-offset: 3px; }
.other-platform svg { width: 18px; height: 18px; }
.version { color: var(--text-dim); text-decoration: none; font-size: 15px; }
.version:hover { color: var(--text); text-decoration: underline; text-underline-offset: 3px; }
.icon-windows rect { fill: var(--windows); }

.status { display: flex; align-items: flex-start; gap: 10px; margin: 0 0 20px; font-size: 15px; }
.status svg { width: 18px; height: 18px; margin-top: 3px; }
.download .status { margin: 6px 0 0; }
.status-beta svg { color: var(--beta); }
.status-off { color: var(--text-dim); }
.status-off svg { color: var(--text-dim); }
.status-warn svg { color: var(--warn); }

.demo { margin: 0; background: var(--surface); border-radius: 18px; padding: 28px 24px 22px; }
@media (min-width: 720px) { .demo { padding: 40px 36px 28px; } }
.field {
  background: var(--base); border-radius: 12px; padding: 18px 20px;
  font-size: clamp(17px, 2.2vw, 21px); line-height: 1.5;
  /* Room for two lines, so the panel does not grow while the sentence wraps. */
  min-height: calc(3em + 36px);
}
.typed { white-space: pre-wrap; }
.caret { display: inline-block; width: 2px; height: 1.2em; vertical-align: -0.2em; background: var(--accent); margin-left: 1px; animation: blink 1s steps(1) infinite; }
@keyframes blink { 50% { opacity: 0; } }
@media (prefers-reduced-motion: reduce) { .caret { animation: none; } }
.keys { display: flex; align-items: center; gap: 10px; margin-top: 16px; color: var(--text-dim); font-size: 14px; min-height: 22px; }
.keys svg { width: 18px; height: 18px; }
.keys-typed { white-space: pre-wrap; }

section.install, section.files { padding: 0 0 72px; }
.section-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px 24px; margin-bottom: 24px; }
h2 { font-size: 28px; line-height: 1.2; letter-spacing: -0.01em; font-weight: 600; margin: 0; }
.tabs { display: flex; gap: 4px; user-select: none; }
.tabs button {
  display: inline-flex; align-items: center; gap: 8px;
  font: 500 15px/1 inherit; font-family: inherit; color: var(--text-dim);
  background: transparent; border: 0; border-radius: 8px; padding: 9px 14px; cursor: pointer;
}
.tabs button svg { width: 16px; height: 16px; }
.tabs button:hover { color: var(--text); }
.tabs button[aria-selected="true"] { color: var(--text); background: var(--surface-strong); }
.tab-panel { max-width: 760px; }

.steps { list-style: none; counter-reset: step; margin: 0; padding: 0; display: grid; gap: 20px; }
.steps li { counter-increment: step; display: grid; grid-template-columns: 36px 1fr; column-gap: 14px; }
.steps li::before {
  content: counter(step); grid-row: span 2;
  font: 600 15px/32px "JetBrains Mono", monospace; text-align: center;
  width: 32px; height: 32px; border-radius: 50%; background: var(--surface-strong);
}
.steps p { margin: 4px 0 0; }
.steps .code { grid-column: 2; margin-top: 10px; }
kbd { font-size: 0.85em; background: var(--surface-strong); border-radius: 6px; padding: 1px 7px; }
.code {
  display: flex; align-items: center; gap: 8px; background: var(--surface); border-radius: 10px;
  padding: 6px 6px 6px 16px; min-width: 0;
}
.code code { flex: 1; min-width: 0; font-size: 14px; white-space: pre-wrap; overflow-wrap: anywhere; padding: 8px 0; }
.nowrap { white-space: nowrap; }
.copy { display: inline-grid; place-items: center; width: 38px; height: 38px; border: 0; border-radius: 8px; background: transparent; color: var(--text-dim); cursor: pointer; flex: none; }
.copy:hover { color: var(--text); background: var(--surface-strong); }
.copy svg { width: 18px; height: 18px; grid-area: 1 / 1; transition: opacity 140ms ease-out, transform 140ms ease-out; }
.copy .icon-check { opacity: 0; transform: scale(0.6); color: #3f9d5c; }
.copy.copied .icon-copy { opacity: 0; transform: scale(0.6); }
.copy.copied .icon-check { opacity: 1; transform: none; }
.alternative { margin-top: 32px; }
.alternative p { margin: 0 0 10px; color: var(--text-dim); }

.file-groups { display: grid; gap: 28px; margin-bottom: 24px; }
@media (min-width: 880px) { .file-groups { grid-template-columns: 1.2fr 1fr 1.2fr 1fr; gap: 28px; } }
.file-group h3 { display: flex; align-items: center; gap: 10px; font-size: 16px; font-weight: 600; margin: 0 0 12px; }
.file-group h3 svg { width: 18px; height: 18px; }
.file-group ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
.file-group li { display: grid; gap: 0; padding: 8px 12px; margin: 0 -12px; border-radius: 10px; }
.file-group li:hover { background: var(--surface); }
.file-group a { display: inline-flex; align-items: center; gap: 10px; text-decoration: none; font-size: 14px; color: var(--accent-text); overflow-wrap: anywhere; }
.file-group a svg { width: 16px; height: 16px; }
.file-detail { font-size: 14px; color: var(--text-dim); padding-left: 26px; }

.bottom { padding-top: 8px; padding-bottom: 48px; }
.social { display: flex; flex-wrap: wrap; gap: 4px 8px; margin: 0 -10px 12px; }
.social a { display: inline-flex; align-items: center; gap: 8px; padding: 8px 10px; border-radius: 8px; text-decoration: none; color: var(--text-dim); font-size: 15px; }
.social a:hover { color: var(--text); background: var(--surface); }
.social svg { width: 18px; height: 18px; }
.legal { margin: 0; font-size: 14px; color: var(--text-dim); }
a:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
`;
