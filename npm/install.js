#!/usr/bin/env node
// Downloads the pg binary for this computer from the GitHub release of this
// package's version and checks it against SHA256SUMS (packed into the
// package when it was published). Runs after `npm install`; when install
// scripts are turned off, bin/pg runs it on the first call instead.
//
// On Windows it then points npm's pg, pg.cmd and pg.ps1 straight at the
// binary: through Node every call took about half a second more.
"use strict";
const fs = require("fs");
const path = require("path");

const REPO = "Utku4836/Paragma";
const OS = { win32: "windows", linux: "linux", darwin: "darwin" };
const ARCH = { x64: "x64", arm64: "arm64" };

/** The release asset's name for this computer: pg-windows-x64.exe */
function assetName() {
  const os = OS[process.platform];
  const arch = ARCH[process.arch];
  if (!os || !arch) return null;
  return `pg-${os}-${arch}${os === "windows" ? ".exe" : ""}`;
}

/** Where the binary is kept: next to bin/pg, inside the package. */
function binaryPath() {
  const name = assetName();
  return name && path.join(__dirname, "bin", name);
}

function expectedSum(name) {
  const file = path.join(__dirname, "SHA256SUMS");
  if (!fs.existsSync(file)) return null;
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    const [sum, asset] = line.trim().split(/\s+\*?/);
    if (asset === name) return sum;
  }
  return null;
}

function get(url, redirects = 5) {
  const https = require("https");
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "paragma-installer" } }, (res) => {
        if ([301, 302, 303, 307, 308].includes(res.statusCode) && res.headers.location && redirects > 0) {
          res.resume();
          resolve(get(new URL(res.headers.location, url).toString(), redirects - 1));
        } else if (res.statusCode !== 200) {
          res.resume();
          reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        } else {
          resolve(res);
        }
      })
      .on("error", reject);
  });
}

/** Downloads, unpacks and checks the binary; returns its path. */
async function install({ quiet = false } = {}) {
  const name = assetName();
  if (!name) throw new Error(`no pg build for ${process.platform}/${process.arch}`);
  const dest = binaryPath();
  if (fs.existsSync(dest)) return dest;
  const version = require("./package.json").version;
  const url = `https://github.com/${REPO}/releases/download/v${version}/${name}.gz`;
  if (!quiet) process.stderr.write(`paragma: downloading pg ${version} for ${process.platform}/${process.arch}...\n`);
  const res = await get(url);
  const tmp = `${dest}.${process.pid}.tmp`;
  const hash = require("crypto").createHash("sha256");
  await new Promise((resolve, reject) => {
    const gunzip = require("zlib").createGunzip();
    const out = fs.createWriteStream(tmp, { mode: 0o755 });
    gunzip.on("data", (chunk) => hash.update(chunk));
    res.on("error", reject);
    gunzip.on("error", reject);
    out.on("error", reject);
    out.on("finish", resolve);
    res.pipe(gunzip).pipe(out);
  });
  const sum = hash.digest("hex");
  const want = expectedSum(name);
  if (want && sum !== want) {
    fs.rmSync(tmp, { force: true });
    throw new Error(`the downloaded ${name} does not match its checksum`);
  }
  fs.renameSync(tmp, dest);
  if (process.platform !== "win32") fs.chmodSync(dest, 0o755);
  if (!quiet) process.stderr.write("paragma: ready. Connect your coding agents with: pg setup\n");
  return dest;
}

const MARK = "written by paragma (install.js): runs pg itself, not through Node";

/**
 * npm's shims for pg that run it through Node, with what each becomes:
 * pg (Git Bash and other sh), pg.cmd and pg.ps1, in the folder of a global
 * install (<prefix>, the package in <prefix>/node_modules/paragma) or of a
 * local one (node_modules/.bin). A shim is known by the launcher path npm
 * wrote in it; the new one is written as npm writes them and points inside
 * the package too, so npm still treats it as its own: an update writes it
 * anew (the first call then points it again), an uninstall removes it.
 */
function shims(exe) {
  const launcher = path.join(__dirname, "bin", "pg");
  const dirs = [path.resolve(__dirname, "..", ".."), path.resolve(__dirname, "..", ".bin")];
  const found = [];
  for (const dir of dirs) {
    const slash = (p) => path.relative(dir, p).split(path.sep).join("/");
    const back = (p) => path.relative(dir, p).split(path.sep).join("\\");
    const sh = [
      "#!/bin/sh",
      `# ${MARK}`,
      'case "$0" in',
      `  *\\\\*) basedir=$(dirname "$(echo "$0" | sed -e 's,\\\\,/,g')") ;;`,
      "  */*) basedir=${0%/*} ;;",
      "  *) basedir=. ;;",
      "esac",
      `if [ -f "$basedir/${slash(exe)}" ]; then`,
      `  exec "$basedir/${slash(exe)}" "$@"`,
      "fi",
      "# the binary is gone (an update): Node's launcher downloads it again",
      `exec node "$basedir/${slash(launcher)}" "$@"`,
      "",
    ].join("\n");
    const cmd = [
      "@ECHO off",
      `REM ${MARK}`,
      "GOTO start",
      ":find_dp0",
      "SET dp0=%~dp0",
      "EXIT /b",
      ":start",
      "SETLOCAL",
      "CALL :find_dp0",
      `IF NOT EXIST "%dp0%\\${back(exe)}" GOTO node`,
      `endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%dp0%\\${back(exe)}" %*`,
      ":node",
      "REM the binary is gone (an update): Node's launcher downloads it again",
      `endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "node" "%dp0%\\${back(launcher)}" %*`,
      "",
    ].join("\r\n");
    const ps1 = [
      "#!/usr/bin/env pwsh",
      `# ${MARK}`,
      "$basedir=Split-Path $MyInvocation.MyCommand.Definition -Parent",
      "# --done 1,3 is an array here: passed on as 1,3 (as PowerShell passes it",
      "# to a program it runs itself), not as \"1 3\"",
      "$a=@(foreach ($x in $args) { if ($x -is [array]) { $x -join ',' } else { $x } })",
      `if (-not (Test-Path "$basedir/${slash(exe)}")) {`,
      "  # the binary is gone (an update): Node's launcher downloads it again",
      `  & node "$basedir/${slash(launcher)}" @a`,
      "  exit $LASTEXITCODE",
      "}",
      "if ($MyInvocation.ExpectingInput) {",
      `  $input | & "$basedir/${slash(exe)}" @a`,
      "} else {",
      `  & "$basedir/${slash(exe)}" @a`,
      "}",
      "exit $LASTEXITCODE",
      "",
    ].join("\n");
    const files = [
      ["pg", `"$basedir/${slash(launcher)}" "$@"`, sh],
      ["pg.cmd", `"%dp0%\\${back(launcher)}" %*`, cmd],
      ["pg.ps1", `"$basedir/${slash(launcher)}" $args`, ps1],
    ];
    for (const [name, viaNode, text] of files) {
      const file = path.join(dir, name);
      let now;
      try {
        now = fs.readFileSync(file, "utf8");
      } catch {
        continue;
      }
      if (now.includes(viaNode)) found.push({ file, text });
    }
  }
  return found;
}

/** Points npm's shims at the binary (Windows); how many were changed. */
function direct(exe) {
  if (process.platform !== "win32" || !exe || !fs.existsSync(exe)) return 0;
  let changed = 0;
  for (const { file, text } of shims(exe)) {
    const tmp = `${file}.${process.pid}.tmp`;
    try {
      fs.writeFileSync(tmp, text);
      fs.renameSync(tmp, file);
      changed++;
    } catch {
      fs.rmSync(tmp, { force: true });
    }
  }
  return changed;
}

module.exports = { install, binaryPath, assetName, direct };

if (require.main === module) {
  install()
    .then(direct)
    .catch((e) => {
      // the install itself does not fail: bin/pg tries again on the first call
      process.stderr.write(`paragma: ${e.message}; pg will try again when it first runs.\n`);
    });
}
