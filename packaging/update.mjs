// Writes the Homebrew cask and the winget manifests for a release.
// Usage: node packaging/update.mjs <version> <dir with the release assets>
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [version, dir] = process.argv.slice(2);
if (!version || !dir) throw new Error("usage: update.mjs <version> <assets dir>");
const files = readdirSync(dir);
const sha = (pattern) => {
  const f = files.find((x) => pattern.test(x));
  if (!f) throw new Error(`no asset matches ${pattern}`);
  return createHash("sha256").update(readFileSync(join(dir, f))).digest("hex");
};
const base = `https://github.com/0xgle/whylag/releases/download/v${version}`;

// Homebrew: `brew tap 0xgle/whylag https://github.com/0xgle/whylag`
mkdirSync("Casks", { recursive: true });
writeFileSync(
  "Casks/whylag.rb",
  `cask "whylag" do
  arch arm: "aarch64", intel: "x64"

  version "${version}"
  sha256 arm:   "${sha(/_aarch64\.dmg$/)}",
         intel: "${sha(/_x64\.dmg$/)}"

  url "${base.replace(version, "#{version}")}/WhyLag_#{version}_#{arch}.dmg"
  name "WhyLag"
  desc "Network diagnostics, Wi-Fi survey and troubleshooting"
  homepage "https://0xgle.github.io/whylag/"

  app "WhyLag.app"

  zap trash: [
    "~/Library/Application Support/eu.m8.whylag",
    "~/Library/Caches/eu.m8.whylag",
  ]
end
`,
);

// winget: manifests for microsoft/winget-pkgs (manifests/m/M8/WhyLag/<version>).
const id = "M8.WhyLag";
const out = join("packaging", "winget", version);
mkdirSync(out, { recursive: true });
const header = (type) => `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${type}.1.6.0.schema.json\n\nPackageIdentifier: ${id}\nPackageVersion: ${version}\n`;
writeFileSync(join(out, `${id}.yaml`), `${header("version")}DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.6.0\n`);
writeFileSync(
  join(out, `${id}.installer.yaml`),
  `${header("installer")}InstallerType: nullsoft
Scope: user
InstallModes:
  - interactive
  - silent
UpgradeBehavior: install
Installers:
  - Architecture: x64
    InstallerUrl: ${base}/WhyLag_${version}_x64-setup.exe
    InstallerSha256: ${sha(/_x64-setup\.exe$/).toUpperCase()}
ManifestType: installer
ManifestVersion: 1.6.0
`,
);
writeFileSync(
  join(out, `${id}.locale.en-US.yaml`),
  `${header("defaultLocale")}PackageLocale: en-US
Publisher: M8 Mateusz Glegoła
PublisherUrl: https://github.com/0xgle
PackageName: WhyLag
PackageUrl: https://0xgle.github.io/whylag/
License: Proprietary
Copyright: © 2026 M8 Mateusz Glegoła
ShortDescription: Find out why your network lags. Wi-Fi analyzer, heatmap site survey, device discovery, speed, path and security tests.
Tags:
  - wifi
  - network
  - speed-test
  - diagnostics
  - heatmap
ReleaseNotesUrl: https://github.com/0xgle/whylag/releases/tag/v${version}
ManifestType: defaultLocale
ManifestVersion: 1.6.0
`,
);
console.log(`cask and winget manifests written for ${version}`);
