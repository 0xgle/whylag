import { useEffect, useState, type ReactNode } from "react";
import { AlertTriangle, Info, OctagonAlert } from "lucide-react";
import { quality, qualityColor, rssiFraction } from "../lib/signal";

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  label?: string;
}) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Badge({ tone, children, title }: { tone?: "crit" | "warn" | "ok" | "info"; children: ReactNode; title?: string }) {
  return (
    <span className="badge" data-tone={tone} title={title}>
      {children}
    </span>
  );
}

export function SignalBar({ rssi, width = 64 }: { rssi: number; width?: number }) {
  const q = quality(rssi);
  return (
    <span className="sigbar">
      <span className="sigbar-track" style={{ width }}>
        <span className="sigbar-fill" style={{ width: `${rssiFraction(rssi) * 100}%`, background: qualityColor[q] }} />
      </span>
      <span style={{ minWidth: 30, textAlign: "right" }}>{rssi}</span>
    </span>
  );
}

export function Notice({
  tone = "info",
  children,
  action,
}: {
  tone?: "info" | "warn" | "crit";
  children: ReactNode;
  action?: ReactNode;
}) {
  const Icon = tone === "crit" ? OctagonAlert : tone === "warn" ? AlertTriangle : Info;
  return (
    <div className="notice" data-tone={tone} role={tone === "crit" ? "alert" : "status"}>
      <Icon size={16} aria-hidden />
      <div>{children}</div>
      {action}
    </div>
  );
}

export function Panel({
  title,
  actions,
  children,
  bodyClass = "panel-body",
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  bodyClass?: string;
  className?: string;
}) {
  return (
    <section className={`panel ${className ?? ""}`}>
      {(title || actions) && (
        <header className="panel-head">
          {title && <h2 className="panel-title">{title}</h2>}
          {actions && <div className="row">{actions}</div>}
        </header>
      )}
      <div className={bodyClass}>{children}</div>
    </section>
  );
}

/** Tiny line chart for header readouts. Values in dBm (or any number). */
export function Sparkline({
  values,
  width = 96,
  height = 30,
  min = -95,
  max = -30,
  color = "var(--accent)",
}: {
  values: number[];
  width?: number;
  height?: number;
  min?: number;
  max?: number;
  color?: string;
}) {
  if (values.length < 2) return <svg width={width} height={height} className="readout-spark" aria-hidden />;
  const span = Math.max(1, max - min);
  const step = width / (values.length - 1);
  const pts = values.map((v, i) => `${(i * step).toFixed(1)},${(height - 2 - ((Math.min(max, Math.max(min, v)) - min) / span) * (height - 4)).toFixed(1)}`);
  return (
    <svg width={width} height={height} className="readout-spark" aria-hidden>
      <polyline points={pts.join(" ")} fill="none" stroke={color} strokeWidth={1.75} strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}

export function useToast(): [string | null, (msg: string) => void] {
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    if (!msg) return;
    const id = setTimeout(() => setMsg(null), 1800);
    return () => clearTimeout(id);
  }, [msg]);
  return [msg, setMsg];
}

export function Toast({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <div className="toast" role="status">
      {message}
    </div>
  );
}

export function Field({ label, help, children }: { label: ReactNode; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="field">
      <div className="field-label">{label}</div>
      {help && <div className="field-help">{help}</div>}
      <div className="field-control">{children}</div>
    </div>
  );
}
