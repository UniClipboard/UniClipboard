#!/usr/bin/env node
// Assemble publishable npm packages from the CLI release archives.
//
// Input:  the `uniclipboard-cli-<version>-<target>.{tar.gz,zip}` archives
//         produced by build-cli.yml (each contains `uniclip` + `uniclipd`).
// Output: <out>/cli-<platform>-<arch>/  — 5 platform packages with binaries
//         <out>/uniclipboard/          — main package with the JS launcher
//
// Platform packages MUST ship `uniclip` and `uniclipd` side by side in bin/:
// `uniclip start` resolves the daemon as a sibling of current_exe()
// (ADR-008 D13). The main package pins platform packages to the exact same
// version so a partially-upgraded install can never mix versions.
//
// Usage:
//   node scripts/build-npm-packages.mjs \
//     --version 0.14.1 --artifacts-dir release-assets --out npm-dist

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const TARGETS = [
  { rust: "aarch64-apple-darwin", node: "darwin-arm64", os: "darwin", cpu: "arm64", ext: "tar.gz" },
  { rust: "x86_64-apple-darwin", node: "darwin-x64", os: "darwin", cpu: "x64", ext: "tar.gz" },
  { rust: "aarch64-unknown-linux-musl", node: "linux-arm64", os: "linux", cpu: "arm64", ext: "tar.gz" },
  { rust: "x86_64-unknown-linux-musl", node: "linux-x64", os: "linux", cpu: "x64", ext: "tar.gz" },
  { rust: "x86_64-pc-windows-msvc", node: "win32-x64", os: "win32", cpu: "x64", ext: "zip" },
];

function parseArgs(argv) {
  const args = {};
  for (let i = 2; i < argv.length; i++) {
    const flag = argv[i];
    if (!flag.startsWith("--")) fail(`unexpected argument: ${flag}`);
    args[flag.slice(2)] = argv[++i];
  }
  return args;
}

function fail(msg) {
  console.error(`build-npm-packages: ${msg}`);
  process.exit(1);
}

const { version, "artifacts-dir": artifactsDir, out: outDir } = parseArgs(process.argv);

if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  fail(`--version is required and must be a valid semver version (got "${version}")`);
}
if (!artifactsDir || !fs.existsSync(artifactsDir)) {
  fail(`--artifacts-dir is required and must exist (got "${artifactsDir}")`);
}
if (!outDir) {
  fail("--out is required");
}

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });

const licenseSrc = path.join(REPO_ROOT, "LICENSE");

// --- Platform packages -----------------------------------------------------

for (const target of TARGETS) {
  const archive = path.join(
    artifactsDir,
    `uniclipboard-cli-${version}-${target.rust}.${target.ext}`
  );
  if (!fs.existsSync(archive)) {
    fail(`missing CLI archive for ${target.rust}: ${archive}`);
  }

  const pkgName = `@uniclipboard/cli-${target.node}`;
  const pkgDir = path.join(outDir, `cli-${target.node}`);
  const binDir = path.join(pkgDir, "bin");
  fs.mkdirSync(binDir, { recursive: true });

  if (target.ext === "zip") {
    execFileSync("unzip", ["-o", "-q", archive, "-d", binDir]);
  } else {
    execFileSync("tar", ["-xzf", archive, "-C", binDir]);
  }

  const exeSuffix = target.os === "win32" ? ".exe" : "";
  for (const bin of ["uniclip", "uniclipd"]) {
    const binPath = path.join(binDir, `${bin}${exeSuffix}`);
    if (!fs.existsSync(binPath)) {
      fail(`archive ${path.basename(archive)} does not contain ${bin}${exeSuffix}`);
    }
    if (!exeSuffix) fs.chmodSync(binPath, 0o755);
  }

  fs.writeFileSync(
    path.join(pkgDir, "package.json"),
    JSON.stringify(
      {
        name: pkgName,
        version,
        description: `UniClipboard CLI binaries (uniclip + uniclipd) for ${target.node}. Install the "uniclipboard" package instead of this one.`,
        homepage: "https://github.com/UniClipboard/UniClipboard",
        repository: {
          type: "git",
          url: "git+https://github.com/UniClipboard/UniClipboard.git",
        },
        license: "AGPL-3.0-only",
        // Yarn PnP: binaries must exist on the real filesystem to be spawned.
        preferUnplugged: true,
        os: [target.os],
        cpu: [target.cpu],
        files: ["bin"],
      },
      null,
      2
    ) + "\n"
  );

  if (fs.existsSync(licenseSrc)) {
    fs.copyFileSync(licenseSrc, path.join(pkgDir, "LICENSE"));
  }

  console.log(`assembled ${pkgName}@${version} from ${path.basename(archive)}`);
}

// --- Main package ----------------------------------------------------------

const mainSrc = path.join(REPO_ROOT, "npm", "uniclipboard");
const mainDir = path.join(outDir, "uniclipboard");
fs.cpSync(mainSrc, mainDir, { recursive: true });

const mainPkgPath = path.join(mainDir, "package.json");
const mainPkg = JSON.parse(fs.readFileSync(mainPkgPath, "utf8"));
mainPkg.version = version;
for (const dep of Object.keys(mainPkg.optionalDependencies)) {
  // Exact pin — no ^/~ — so the main package and platform packages can never
  // resolve to different versions.
  mainPkg.optionalDependencies[dep] = version;
}
const declared = Object.keys(mainPkg.optionalDependencies).sort();
const assembled = TARGETS.map((t) => `@uniclipboard/cli-${t.node}`).sort();
if (JSON.stringify(declared) !== JSON.stringify(assembled)) {
  fail(
    `optionalDependencies in npm/uniclipboard/package.json (${declared.join(", ")}) ` +
      `do not match assembled platform packages (${assembled.join(", ")})`
  );
}
fs.writeFileSync(mainPkgPath, JSON.stringify(mainPkg, null, 2) + "\n");

if (fs.existsSync(licenseSrc)) {
  fs.copyFileSync(licenseSrc, path.join(mainDir, "LICENSE"));
}

console.log(`assembled uniclipboard@${version}`);

// Publish order matters: platform packages first, the main package last, so
// there is no window where `npm install uniclipboard` resolves but its
// optional dependencies 404.
const order = [...TARGETS.map((t) => `cli-${t.node}`), "uniclipboard"];
fs.writeFileSync(path.join(outDir, "publish-order.txt"), order.join("\n") + "\n");
console.log(`publish order written to ${path.join(outDir, "publish-order.txt")}`);
