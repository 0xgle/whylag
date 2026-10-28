// Builds the updater feed (latest.json) once, after every platform finished,
// from the signed update packages attached to the release.
// Usage: node packaging/latest-json.mjs <tag> <dir with the .sig files> <asset names file>
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [tag, dir, namesFile] = process.argv.slice(2);
const version = tag.replace(/^v/, "");
const names = readFileSync(namesFile, "utf8").split("\n").filter(Boolean);
const sigs = readdirSync(dir).filter((f) => f.endsWith(".sig"));
const base = `https://github.com/0xgle/whylag/releases/download/${tag}`;

// Platform key → how its update package is named.
const targets = {
  "windows-x86_64": /_x64-setup\.exe$/,
  "darwin-aarch64": /_aarch64\.app\.tar\.gz$/,
  "darwin-x86_64": /_x64\.app\.tar\.gz$/,
  "linux-x86_64": /_amd64\.AppImage$/,
};
const platforms = {};
for (const [key, pattern] of Object.entries(targets)) {
  const asset = names.find((n) => pattern.test(n));
  const sig = asset && sigs.find((s) => s === `${asset}.sig`);
  if (!asset || !sig) {
    console.warn(`no signed update for ${key}`);
    continue;
  }
  platforms[key] = { signature: readFileSync(join(dir, sig), "utf8").trim(), url: `${base}/${encodeURIComponent(asset)}` };
}
if (!Object.keys(platforms).length) throw new Error("no signed update packages found");
writeFileSync("latest.json", JSON.stringify({ version, notes: `WhyLag ${version}`, pub_date: new Date().toISOString(), platforms }, null, 2));
console.log(`latest.json for ${Object.keys(platforms).join(", ")}`);
