// Deploys the ranking API: creates the D1 database on first run, applies the
// schema, deploys the Worker and writes its URL to web/public/api.json so the
// web test knows where to send results. Needs CLOUDFLARE_API_TOKEN and
// CLOUDFLARE_ACCOUNT_ID in the environment.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const run = (args) => execFileSync("npx", ["wrangler", ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });

const list = JSON.parse(run(["d1", "list", "--json"]));
let db = list.find((d) => d.name === "whylag");
if (!db) {
  run(["d1", "create", "whylag"]);
  db = JSON.parse(run(["d1", "list", "--json"])).find((d) => d.name === "whylag");
}
if (!db) throw new Error("could not create the D1 database");
const id = db.uuid ?? db.id;

const toml = readFileSync("wrangler.toml", "utf8").replace(/database_id = "[^"]*"/, `database_id = "${id}"`);
writeFileSync("wrangler.toml", toml);

run(["d1", "execute", "whylag", "--remote", "--file", "schema.sql", "-y"]);
const out = run(["deploy"]);
const url = out.match(/https:\/\/[a-z0-9.-]+\.workers\.dev/)?.[0];
if (!url) throw new Error(`no workers.dev URL in deploy output:\n${out}`);
writeFileSync("../web/public/api.json", JSON.stringify({ base: url }, null, 2) + "\n");
console.log(`deployed ${url}`);
