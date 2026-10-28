# WhyLag

**Find out why your network lags.** WhyLag is a network diagnostics suite for Windows, macOS and Linux: Wi-Fi analysis, heatmap site surveys, device discovery, performance and path testing, security checks and continuous monitoring, with a plain-language diagnosis in English and Polish.

It does what dedicated handheld testers do for Wi-Fi and IP networks, on any laptop or tablet you already have, and explains the results instead of just showing numbers.

## Modules

| Module | What it does |
|---|---|
| **Overview** | One-click checkup that runs every test, grades the network A–F, names the bottleneck (Wi-Fi, provider, router, DNS, security) and lists fixes in priority order. Exports a shareable result card. |
| **Wi-Fi** | Live scan of every access point with details Windows does not show: channel width, Wi-Fi 4/5/6/7, WPA2/WPA3 and transition mode, PMF, WPS, 802.11k/v/r, AP client count and airtime (BSS Load). Channel graphs for 2.4/5/6 GHz and signal history. |
| **Survey** | Load a floor plan, set the scale and walk the site. Point and continuous walk modes. Heatmaps for signal, second AP, SNR, AP count, co-channel interference, serving AP and throughput. Predictive mode with walls (material losses per band) and virtual APs, automatic channel planning and AP placement suggestions. `.whylag` project files, PNG export. |
| **Discovery** | Finds every device on the subnet (ICMP/TCP sweep and ARP cache, so silent phones show up too), identifies it from vendor, open ports, mDNS, SSDP/UPnP, NetBIOS, HTTP banners and SNMP, and maps switch links from LLDP. |
| **Performance** | Latency, jitter and loss to the router and popular services; DNS resolver benchmark with hijack detection; internet speed test via M-Lab ndt7 with latency under load (bufferbloat grade); LAN throughput test between two computers running WhyLag. |
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

On Windows 11 24H2 and later, Wi-Fi scanning requires *Location → Let desktop apps access your location*. WhyLag detects this and links to the setting.

## Install

Download the installer for your system from [Releases](https://github.com/0xgle/whylag/releases): `.exe`/`.msi` for Windows, `.dmg` for macOS, `.AppImage`/`.deb` for Linux.

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
crates/whylag-core      Rust engine: every scanner, test and analysis, one API dispatcher
crates/whylag-devserver HTTP bridge to the engine for browser development
src-tauri               Desktop shell (Tauri 2): forwards UI calls to the engine
ui                      React + TypeScript interface, EN/PL, light and dark themes
```

Every UI action is a single `method` + JSON `params` call handled by `whylag_core::api::dispatch`, so the desktop app and the browser preview always run identical code. Long tasks (discovery, performance, path, monitoring) run in the background and are polled for progress.

## Releasing

Publish a GitHub release with a tag such as `v0.2.0`. The release workflow builds installers for Windows, macOS (Apple Silicon and Intel) and Linux and attaches them to the release.

## Privacy

All analysis runs locally. The only external services are the ones you start: the speed test uses [M-Lab](https://www.measurementlab.net/), which publishes test results (including the IP address) as open data, and WhyLag asks for consent before the first speed test.

## License

© 2026 M8 Mateusz Glegoła. All rights reserved.

MAC vendor data comes from the IEEE registry via the `oui-data` package (BSD-2-Clause).
