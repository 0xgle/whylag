// Regenerates crates/whylag-core/data/oui.tsv.gz from the `oui-data` npm package
// (IEEE MA-L registry, BSD-2-Clause packaging).
//   npm pack oui-data && tar xzf oui-data-*.tgz && node scripts/build-oui.mjs package/index.json
import { readFileSync, writeFileSync } from "node:fs";
import { gzipSync } from "node:zlib";

const src = process.argv[2];
if (!src) {
  console.error("usage: node scripts/build-oui.mjs <path/to/oui-data/index.json>");
  process.exit(1);
}

const SUFFIX = /\b(co|corp|corporation|inc|incorporated|ltd|limited|llc|l\.l\.c|gmbh|ag|sa|s\.a|srl|bv|b\.v|ab|oy|as|spa|s\.p\.a|kg|plc|pte|pty|sp\. z o\.o|technologies|technology|tech|electronics|electronic|communications|communication|networks|international|industrial|industries|group|holdings|company|systems)\b\.?/gi;

function shorten(raw) {
  let name = raw.split("\n")[0].trim();
  name = name.replace(/\(.*?\)/g, " ");
  for (let i = 0; i < 3; i++) name = name.replace(SUFFIX, " ");
  name = name.replace(/[,.]+/g, " ").replace(/\s+/g, " ").trim();
  if (!name) name = raw.split("\n")[0].trim();
  // Title-case shouting names but keep short acronyms (HP, ZTE, TP-LINK stays readable).
  if (name === name.toUpperCase()) {
    name = name
      .split(" ")
      .map((w) =>
        w
          .split("-")
          .map((part) => (part.length <= 3 ? part : part[0] + part.slice(1).toLowerCase()))
          .join("-"),
      )
      .join(" ");
  }
  return name.slice(0, 40);
}

const data = JSON.parse(readFileSync(src, "utf8"));
const lines = Object.entries(data)
  .filter(([k]) => /^[0-9A-F]{6}$/i.test(k))
  .map(([k, v]) => `${k.toUpperCase()}\t${shorten(v)}`)
  .sort();
const out = new URL("../crates/whylag-core/data/oui.tsv.gz", import.meta.url);
writeFileSync(out, gzipSync(lines.join("\n") + "\n", { level: 9 }));
console.log(`wrote ${lines.length} entries`);
