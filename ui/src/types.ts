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
