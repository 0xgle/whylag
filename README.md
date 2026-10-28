# WhyLag

**Find out why your network lags.** WhyLag is a network diagnostics suite for Windows, macOS, Linux and Android: Wi-Fi analysis, heatmap site surveys, device discovery, performance and path testing, security checks and continuous monitoring, with a plain-language diagnosis in English and Polish.

It does what dedicated handheld testers do for Wi-Fi and IP networks, on any laptop or tablet you already have, and explains the results instead of just showing numbers.

## Modules

| Module | What it does |
|---|---|
| **Overview** | One-click checkup that runs every test, grades the network A–F, names the bottleneck (Wi-Fi, provider, router, DNS, security) and lists fixes in priority order. Exports a shareable result card. |
| **AutoTest** | One button tests every layer (link, DHCP, router, DNS, Wi-Fi, internet, captive portal, your own services) against limit profiles you keep per site. Finds rogue DHCP servers without taking a lease. |
| **Wi-Fi** | Live scan of every access point with details Windows does not show: channel width, Wi-Fi 4/5/6/7, WPA2/WPA3 and transition mode, PMF, WPS, 802.11k/v/r, AP client count and airtime (BSS Load). Channel graphs for 2.4/5/6 GHz and signal history. Walking roaming test: every AP switch with the signal before and after, handover stalls and clients that hold on to weak APs. |
| **Survey** | Load a floor plan, set the scale and walk the site. Point and continuous walk modes. Heatmaps for signal, second AP, SNR, AP count, co-channel interference, serving AP and throughput. Predictive mode with walls (material losses per band) and virtual APs, automatic channel planning and AP placement suggestions. Several floors per project, real APs located from the measurements, AP switches marked on the walk, a printable report of every floor. `.whylag` project files, PNG export. |
| **Discovery** | Finds every device on the subnet (ICMP/TCP sweep and ARP cache, so silent phones show up too), identifies it from vendor, open ports, mDNS, SSDP/UPnP, NetBIOS, HTTP banners and SNMP, and maps switch links from LLDP. |
| **Performance** | Latency, jitter and loss to the router and popular services; DNS resolver benchmark with hijack detection; internet speed test via M-Lab ndt7 with latency under load (bufferbloat grade); LAN throughput test against another WhyLag (with a pairing code) or any iperf3 server. **Gamer mode**: a small always-on-top window with live lag and whether it comes from Wi-Fi, your network or the provider. |
| **Path** | Every hop to a destination with MTR-style loss and latency, labelled as your network, your provider or the internet. Tells real loss from routers that merely rate-limit probes, detects CGNAT. |
| **Security** | Wi-Fi configuration, evil-twin and rogue AP detection, risky services (Telnet, FTP, VNC, unencrypted MQTT…), cameras on the network, UPnP, DNS tampering. Score 0–100. |
| **Monitor** | Samples router, internet, DNS and Wi-Fi every few seconds into SQLite and turns patterns into incidents that say where the problem was. |
| **Reports** | Branded, printable report (save as PDF) with diagnosis, heatmaps, inventory and results, and a complaint letter to the internet provider with timestamps as evidence (PL/EN). |

A built-in **demo office** (42 × 26 m, five APs, a crowded neighbourhood, an evil twin and a coverage hole) lets you try every module without hardware. Switch between *Auto*, *Live only* and *Demo office* in Settings.

## Platform support

| | Wi-Fi scan | Heatmaps | Discovery, performance, path, monitor |
|---|---|---|---|
| **Windows 10/11** | Full (Native Wi-Fi API with raw beacon parsing) | Yes | Yes (native ICMP and traceroute, no admin rights) |
| **Linux** | Full with `iw`, basic with NetworkManager | Yes | Yes |
| **macOS** | Limited (Apple hides BSSIDs from apps) | Yes | Yes |
| **Android** (tablets, phones) | Yes (WifiManager; Android allows a few fresh scans per two minutes) | Yes, walk with a tablet | Yes, except path tracing |

On Windows 11 24H2 and later, Wi-Fi scanning requires *Location → Let desktop apps access your location*. WhyLag detects this and links to the setting.

## Web quick test and provider ranking

`web/` is a zero-install page that runs a speed and lag-under-load test in the browser (M-Lab ndt7), grades the connection, creates a share card and points people to the app. `ranking.html` ranks internet providers per country by the lag people feel under load, from anonymous results (`server/`, a Cloudflare Worker with D1, deployed by `.github/workflows/server.yml` once the `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets exist). It is published to GitHub Pages by `.github/workflows/pages.yml` (enable *Settings → Pages → Source: GitHub Actions* once). Add `?demo` to the URL for a simulated run.

```bash
npm ci --prefix web && npm --prefix web run dev   # http://localhost:5174
```

## Install

Download the installer for your system from [Releases](https://github.com/0xgle/whylag/releases): `.exe` for Windows, `.dmg` for macOS, `.AppImage`/`.deb` for Linux, `.apk` for Android. The desktop app updates itself.

```bash
winget install M8.WhyLag                                   # Windows (once listed in winget)
brew tap 0xgle/whylag https://github.com/0xgle/whylag      # macOS
brew install --cask --no-quarantine whylag
```

The installers are not code-signed yet: on Windows choose *More info → Run anyway*, on macOS right-click the app and choose *Open* (or install with `--no-quarantine` as above).

## Build from source

Requirements: Rust (stable), Node.js 22, and on Linux `libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev`.

```bash
npm ci && npm ci --prefix ui
npm run dev        # desktop app with hot reload
npm run build      # installers in target/release/bundle
```

### Developing in a browser

The UI can run in a normal browser against a local HTTP bridge that serves the very same engine:

```bash
cargo run -p whylag-devserver    # engine on http://127.0.0.1:8787 (localhost only)
npm --prefix ui run dev          # UI on http://localhost:5173
```

### Tests

```bash
cargo test -p whylag-core
npm --prefix ui run typecheck
```

## Architecture

```
web                     Browser quick test and provider ranking (viral entry point), static site
server                  Ranking API (Cloudflare Worker + D1), anonymous results
crates/whylag-core      Rust engine: every scanner, test and analysis, one API dispatcher
crates/whylag-devserver HTTP bridge to the engine for browser development
src-tauri               App shell (Tauri 2, desktop and Android): forwards UI calls to the engine
src-tauri/plugins       Android Wi-Fi scanning (Kotlin, WifiManager)
ui                      React + TypeScript interface, EN/PL, light and dark themes
packaging, Casks        winget manifests and the Homebrew cask, updated by the release workflow
```

Every UI action is a single `method` + JSON `params` call handled by `whylag_core::api::dispatch`, so the desktop app and the browser preview always run identical code. Long tasks (discovery, performance, path, AutoTest, roaming, monitoring) run in the background and are polled for progress.

## Releasing

Run *Actions → Release → Run workflow* with a version such as `v0.2.0` (or publish a GitHub release with that tag). The workflow builds installers for Windows, macOS (Apple Silicon and Intel), Linux and Android, attaches them to the release, signs the update feed (secret `TAURI_SIGNING_PRIVATE_KEY`), and updates the Homebrew cask and winget manifests. With a `WINGET_TOKEN` secret it also opens the winget pull request. A run without a version is a private test build.

## Privacy

All analysis runs locally. The only external services are the ones you start: the speed test uses [M-Lab](https://www.measurementlab.net/), which publishes test results (including the IP address) as open data, and WhyLag asks for consent before the first speed test. The provider ranking is opt-in in the app (on by default in the web test, with a visible checkbox) and stores only the measured numbers plus the provider, country and city that Cloudflare sees for the connection; no IP address or identifier is kept.

## License

© 2026 M8 Mateusz Glegoła. All rights reserved.

MAC vendor data comes from the IEEE registry via the `oui-data` package (BSD-2-Clause).
