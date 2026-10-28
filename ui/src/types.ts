// Mirrors of the Rust core's JSON types (camelCase).

export type Band = "2.4" | "5" | "6";

export type Security =
  | "open"
  | "owe"
  | "wep"
  | "wpa"
  | "wpa2"
  | "wpa2Wpa3"
  | "wpa3"
  | "wpaEnterprise"
  | "wpa2Enterprise"
  | "wpa3Enterprise"
  | "unknown";

export type Pmf = "unknown" | "disabled" | "capable" | "required";

export interface Bss {
  bssid: string;
  ssid: string;
  hidden: boolean;
  rssi: number;
  noise: number | null;
  channel: number;
  frequencyMhz: number;
  band: Band | null;
  widthMhz: number;
  wifiGen: number | null;
  security: Security;
  weakCipher: boolean;
  pmf: Pmf;
  wps: boolean;
  roaming: { k: boolean; v: boolean; r: boolean };
  stationCount: number | null;
  utilizationPct: number | null;
  country: string | null;
  vendor: string | null;
  connected: boolean;
  beaconInterval: number | null;
  lastSeenMs: number;
}

export interface WifiInterface {
  id: string;
  name: string;
  description: string;
  state: string;
  ssid: string | null;
  bssid: string | null;
  rssi: number | null;
  signalQuality: number | null;
  channel: number | null;
  frequencyMhz: number | null;
  band: Band | null;
  txRateMbps: number | null;
  rxRateMbps: number | null;
  phy: string | null;
  wifiGen: number | null;
  security: Security | null;
}

export type ScanSource = "windowsNative" | "linuxIw" | "linuxNmcli" | "macos" | "simulator";

export interface Notice {
  code: string;
  detail: string;
}

export interface ScanResult {
  source: ScanSource;
  interfaces: WifiInterface[];
  networks: Bss[];
  timestampMs: number;
  notices: Notice[];
}

export type DataSource = "auto" | "live" | "simulator";

export interface Settings {
  dataSource: DataSource;
  language: "en" | "pl";
  snmpCommunity: string;
  reportCompany: string;
  reportAuthor: string;
  contractedDownMbps: number | null;
  contractedUpMbps: number | null;
  mlabConsent: boolean;
  coverageThresholdDbm: number;
}

export type Material = "drywall" | "glass" | "wood" | "brick" | "concrete" | "metal" | "shaft";

export interface Wall {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  material: Material;
}

export interface FloorPlan {
  name: string;
  imageDataUrl: string;
  widthPx: number;
  heightPx: number;
  metersPerPx: number;
  wallsPx: Wall[];
}

export interface SimInfo {
  position: [number, number];
  widthM: number;
  heightM: number;
  floorPlan: FloorPlan;
}

export interface AppInfo {
  version: string;
  os: string;
  arch: string;
  dataDir: string;
}

export interface ApiError {
  code: string;
  message: string;
}

// ---------- Survey ----------

export interface Reading {
  bssid: string;
  ssid: string;
  rssi: number;
  channel: number;
  band: Band | null;
  widthMhz: number;
  noise: number | null;
  security: Security;
}

export interface Sample {
  x: number;
  y: number;
  tMs: number;
  readings: Reading[];
  throughputMbps?: number | null;
  latencyMs?: number | null;
}

export interface VirtualRadio {
  band: Band;
  channel: number;
  widthMhz: number;
  eirpDbm: number;
  enabled: boolean;
}

export interface VirtualAp {
  id: string;
  name: string;
  x: number;
  y: number;
  radios: VirtualRadio[];
}

export type Layer = "signal" | "secondSignal" | "snr" | "apCount" | "coChannel" | "bestAp" | "throughput";
export type SurveyMode = "measured" | "predicted";

export interface SurveyProject {
  version: 1;
  name: string;
  createdMs: number;
  floorPlan: { name: string; imageDataUrl: string; widthPx: number; heightPx: number; metersPerPx: number | null };
  samples: Sample[];
  walls: Wall[];
  aps: VirtualAp[];
}

export interface Heatmap {
  cols: number;
  rows: number;
  cellPx: number;
  layer: Layer;
  mode: SurveyMode;
  values: (number | null)[];
  legend: { index: number; label: string; detail: string }[];
  stats: {
    cells: number;
    coveredCells: number;
    coverageRatio: number;
    areaM2: number | null;
    min: number | null;
    max: number | null;
    mean: number | null;
  };
}

export interface ChannelPlan {
  band: Band;
  widthMhz: number;
  assignments: { apId: string; apName: string; from: number | null; to: number }[];
  costBefore: number;
  costAfter: number;
}

export interface PlacementSuggestion {
  x: number;
  y: number;
  coverageBefore: number;
  coverageAfter: number;
}

// ---------- Network ----------

export interface LocalNet {
  interfaceName: string;
  friendlyName: string | null;
  description: string | null;
  kind: string;
  mac: string | null;
  ipv4: string | null;
  prefixLen: number | null;
  ipv6: string[];
  gateway: string | null;
  gatewayMac: string | null;
  dnsServers: string[];
  linkSpeedMbps: number | null;
  mtu: number | null;
  dhcp: boolean | null;
}

export type DeviceKind =
  | "router"
  | "switch"
  | "accessPoint"
  | "firewall"
  | "server"
  | "nas"
  | "printer"
  | "camera"
  | "phone"
  | "tablet"
  | "computer"
  | "tv"
  | "mediaPlayer"
  | "speaker"
  | "iot"
  | "gameConsole"
  | "voip"
  | "unknown";

export interface Host {
  ip: string;
  mac: string | null;
  vendor: string | null;
  randomMac: boolean;
  name: string | null;
  names: [string, string][];
  kind: DeviceKind;
  evidence: string[];
  model: string | null;
  os: string | null;
  ports: { port: number; service: string }[];
  mdnsServices: string[];
  httpTitle: string | null;
  httpServer: string | null;
  snmpName: string | null;
  snmpDescr: string | null;
  upnpType: string | null;
  rttMs: number | null;
  isGateway: boolean;
  isSelf: boolean;
}

export interface Link {
  from: string;
  localPort: string | null;
  neighbor: string;
  neighborIp: string | null;
  remotePort: string | null;
}

interface JobBase {
  running: boolean;
  phase: string;
  progress: number;
  startedMs: number;
  finishedMs: number | null;
  simulated: boolean;
  error: string | null;
}

export interface DiscoveryRun extends JobBase {
  local: LocalNet | null;
  subnet: string | null;
  hosts: Host[];
  links: Link[];
}

export interface LatencyStats {
  id: string;
  label: string;
  host: string;
  kind: "icmp" | "tcp" | null;
  group: string;
  sent: number;
  received: number;
  lossPct: number;
  min: number | null;
  avg: number | null;
  median: number | null;
  p95: number | null;
  max: number | null;
  jitter: number | null;
  samples: (number | null)[];
}

export interface DnsResult {
  server: string;
  label: string;
  system: boolean;
  medianMs: number | null;
  succeeded: number;
  total: number;
}

export interface SpeedResult {
  server: string;
  downloadMbps: number | null;
  uploadMbps: number | null;
  idleLatencyMs: number | null;
  loadedDownLatencyMs: number | null;
  loadedUpLatencyMs: number | null;
  grade: string | null;
  downSeries: [number, number][];
  upSeries: [number, number][];
}

export interface PerfRun extends JobBase {
  latency: LatencyStats[];
  dns: DnsResult[];
  dnsHijack: boolean | null;
  speed: SpeedResult | null;
  speedError: string | null;
}

export type Segment = "lan" | "isp" | "internet" | "destination" | "unknown";

export interface Hop {
  ttl: number;
  ip: string | null;
  name: string | null;
  segment: Segment;
  sent: number;
  received: number;
  lossPct: number;
  best: number | null;
  avg: number | null;
  worst: number | null;
  last: number | null;
  jitter: number | null;
}

export interface PathFinding {
  code: string;
  ttl: number | null;
  segment: Segment | null;
  value: number | null;
}

export interface PathRun extends JobBase {
  target: string;
  targetIp: string | null;
  hops: Hop[];
  findings: PathFinding[];
  rounds: number;
}
