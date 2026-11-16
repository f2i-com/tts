// Assembles a self-contained static site bundle in `dist/` ready to upload directly to any
// static host (Cloudflare Pages, Netlify, Vercel, S3, Nginx/Apache root, GitHub Pages, etc.).
//
//   npm run build
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkgDir = path.join(root, "pkg");
const demoDir = path.join(root, "demo");
const profilesDir = path.join(root, "profiles");
const distDir = path.join(root, "dist");

if (!fs.existsSync(path.join(pkgDir, "tts.wasm"))) {
  execFileSync(process.execPath, [path.join(root, "tools", "build_wasm.mjs")], {
    cwd: root,
    stdio: "inherit",
  });
}

fs.rmSync(distDir, { recursive: true, force: true });
fs.mkdirSync(path.join(distDir, "profiles"), { recursive: true });

// 1. Copy WebAssembly engine + ES module loader + TypeScript types to root of dist/
for (const file of ["tts.wasm", "tts.js", "tts.d.ts"]) {
  fs.copyFileSync(path.join(pkgDir, file), path.join(distDir, file));
}

// 2. Copy speaker profiles (`./profiles/*.json`)
for (const entry of fs.readdirSync(profilesDir)) {
  if (entry.endsWith(".json")) {
    fs.copyFileSync(path.join(profilesDir, entry), path.join(distDir, "profiles", entry));
  }
}

// 3. Copy demo HTML + CSS, and rewrite `../pkg/` & `../profiles/` to `./` in `dist/app.js`
fs.copyFileSync(path.join(demoDir, "index.html"), path.join(distDir, "index.html"));
fs.copyFileSync(path.join(demoDir, "styles.css"), path.join(distDir, "styles.css"));

const appSrc = fs.readFileSync(path.join(demoDir, "app.js"), "utf8");
const distApp = appSrc
  .replace('from "../pkg/tts.js"', 'from "./tts.js"')
  .replace("fetch(`../profiles/${id}.json`)", "fetch(`./profiles/${id}.json`)");

if (distApp.includes("../pkg/") || distApp.includes("../profiles/")) {
  throw new Error("Unrewritten parent-relative path remained in dist/app.js");
}
fs.writeFileSync(path.join(distDir, "app.js"), distApp);

// Syntax-check generated dist/app.js
execFileSync(process.execPath, ["--check", path.join(distDir, "app.js")], { stdio: "inherit" });

// 4. Write `_headers` for Cloudflare Pages / Netlify (proper WASM MIME + CORS for embedding)
fs.writeFileSync(
  path.join(distDir, "_headers"),
  `/tts.wasm
  Content-Type: application/wasm
  Access-Control-Allow-Origin: *
  Cache-Control: public, max-age=3600

/tts.js
  Content-Type: text/javascript; charset=utf-8
  Access-Control-Allow-Origin: *
`
);

const totalBytes = fs
  .readdirSync(distDir, { recursive: true, withFileTypes: true })
  .filter((d) => d.isFile())
  .reduce((sum, d) => sum + fs.statSync(path.join(d.parentPath || d.path, d.name)).size, 0);

console.log(
  `[tts] Built upload-ready static site in dist/ (${(totalBytes / (1024 * 1024)).toFixed(2)} MB total across index.html, app.js, styles.css, tts.wasm, tts.js, tts.d.ts, profiles/*.json)`
);
