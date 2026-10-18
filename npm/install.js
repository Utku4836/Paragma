// Downloads the pg binary for this platform from the GitHub release that
// matches this package's version. Release assets are named
// pg-<os>-<arch>[.exe], for example pg-windows-amd64.exe or pg-darwin-arm64.
const fs = require("fs");
const https = require("https");
const path = require("path");

const { version } = require("./package.json");
const REPO = "Utku4836/Paragma";

const os = { win32: "windows", linux: "linux", darwin: "darwin" }[process.platform];
const arch = { x64: "amd64", arm64: "arm64" }[process.arch];
if (!os || !arch) {
  console.error(`paragma: no prebuilt pg for ${process.platform}/${process.arch}. Build it with: go install github.com/${REPO}/cmd/pg@latest`);
  process.exit(0);
}
const asset = `pg-${os}-${arch}${os === "windows" ? ".exe" : ""}`;
const url = `https://github.com/${REPO}/releases/download/v${version}/${asset}`;
const binDir = path.join(__dirname, "bin");
const target = path.join(binDir, os === "windows" ? "pg.exe" : "pg-native");

function get(u, redirects, done) {
  https
    .get(u, { headers: { "User-Agent": "paragma-npm" } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location && redirects > 0) {
        res.resume();
        return get(res.headers.location, redirects - 1, done);
      }
      if (res.statusCode !== 200) {
        res.resume();
        return done(new Error(`HTTP ${res.statusCode} for ${u}`));
      }
      const tmp = target + ".download";
      const out = fs.createWriteStream(tmp, { mode: 0o755 });
      res.pipe(out);
      out.on("finish", () => out.close(() => done(null, tmp)));
      out.on("error", done);
    })
    .on("error", done);
}

get(url, 5, (err, tmp) => {
  if (err) {
    console.error(`paragma: could not download ${asset}: ${err.message}`);
    console.error(`Download it from https://github.com/${REPO}/releases and put it on your PATH as pg.`);
    process.exit(0); // do not fail the whole npm install
  }
  fs.renameSync(tmp, target);
  if (os !== "windows") {
    // the npm bin link points at bin/pg: make it the binary itself, so pg
    // starts without Node in between
    fs.chmodSync(target, 0o755);
    fs.renameSync(target, path.join(binDir, "pg"));
  }
  console.log(`paragma: installed pg ${version} (${os}/${arch}). Next: pg setup`);
});
