#!/usr/bin/env node
// Downloads the pg binary for this computer from the GitHub release of this
// package's version and checks it against SHA256SUMS (packed into the
// package when it was published). Runs after `npm install`; when install
// scripts are turned off, bin/pg runs it on the first call instead.
"use strict";
const crypto = require("crypto");
const fs = require("fs");
const https = require("https");
const path = require("path");
const zlib = require("zlib");

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
  const hash = crypto.createHash("sha256");
  await new Promise((resolve, reject) => {
    const gunzip = zlib.createGunzip();
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

module.exports = { install, binaryPath, assetName };

if (require.main === module) {
  install().catch((e) => {
    // the install itself does not fail: bin/pg tries again on the first call
    process.stderr.write(`paragma: ${e.message}; pg will try again when it first runs.\n`);
  });
}
