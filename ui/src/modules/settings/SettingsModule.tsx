import { useEffect, useState } from "react";
import { useApp, type Theme } from "../../store";
import { useT } from "../../i18n";
import type { DataSource, Settings } from "../../types";
import { Field, Panel, Segmented, Toast, useToast } from "../../components/ui";

function NumberInput({ value, onCommit, placeholder, width = 120 }: { value: number | null; onCommit: (v: number | null) => void; placeholder?: string; width?: number }) {
  const [text, setText] = useState(value == null ? "" : String(value));
  useEffect(() => setText(value == null ? "" : String(value)), [value]);
  return (
    <input
      className="input"
      style={{ width }}
      inputMode="decimal"
      value={text}
      placeholder={placeholder}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const n = parseFloat(text.replace(",", "."));
        onCommit(Number.isFinite(n) ? n : null);
      }}
    />
  );
}

function TextInput({ value, onCommit, width = 260 }: { value: string; onCommit: (v: string) => void; width?: number }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return <input className="input" style={{ width }} value={text} onChange={(e) => setText(e.target.value)} onBlur={() => text !== value && onCommit(text)} />;
}

export function SettingsModule() {
  const t = useT();
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.updateSettings);
  const theme = useApp((s) => s.theme);
  const setTheme = useApp((s) => s.setTheme);
  const info = useApp((s) => s.appInfo);
  const [toast, showToast] = useToast();

  if (!settings) return <div className="empty">{t("common.loading")}</div>;

  const save = async (patch: Partial<Settings>) => {
    await update(patch);
    showToast(t("settings.saved"));
  };

  return (
    <div className="stack" style={{ maxWidth: 760 }}>
      <Panel>
        <div className="stack">
          <Field label={t("settings.dataSource")} help={t("settings.dataSource.help")}>
            <Segmented<DataSource>
              value={settings.dataSource}
              onChange={(v) => save({ dataSource: v })}
              options={[
                { value: "auto", label: t("settings.dataSource.auto") },
                { value: "live", label: t("settings.dataSource.live") },
                { value: "simulator", label: t("settings.dataSource.simulator") },
              ]}
            />
          </Field>
          <Field label={t("settings.language")}>
            <Segmented<"en" | "pl">
              value={settings.language}
              onChange={(v) => save({ language: v })}
              options={[
                { value: "en", label: "English" },
                { value: "pl", label: "Polski" },
              ]}
            />
          </Field>
          <Field label={t("settings.theme")}>
            <Segmented<Theme>
              value={theme}
              onChange={setTheme}
              options={[
                { value: "system", label: t("settings.theme.system") },
                { value: "light", label: t("settings.theme.light") },
                { value: "dark", label: t("settings.theme.dark") },
              ]}
            />
          </Field>
        </div>
      </Panel>

      <Panel>
        <div className="stack">
          <Field label={t("settings.coverage")} help={t("settings.coverage.help")}>
            <div className="row">
              <NumberInput value={settings.coverageThresholdDbm} onCommit={(v) => v != null && save({ coverageThresholdDbm: Math.round(v) })} width={90} />
              <span className="muted">dBm</span>
            </div>
          </Field>
          <Field label={t("settings.isp")} help={t("settings.isp.help")}>
            <div className="row">
              <label className="row" style={{ gap: 6 }}>
                <span className="muted small">{t("settings.isp.down")}</span>
                <NumberInput value={settings.contractedDownMbps} onCommit={(v) => save({ contractedDownMbps: v })} width={100} />
              </label>
              <label className="row" style={{ gap: 6 }}>
                <span className="muted small">{t("settings.isp.up")}</span>
                <NumberInput value={settings.contractedUpMbps} onCommit={(v) => save({ contractedUpMbps: v })} width={100} />
              </label>
            </div>
          </Field>
          <Field label={t("settings.ranking")} help={t("settings.ranking.help")}>
            <label className="check">
              <input type="checkbox" checked={!!settings.shareRanking} onChange={(e) => save({ shareRanking: e.target.checked })} />
              {t("settings.ranking.share")}
            </label>
          </Field>
          <Field label={t("settings.snmp")} help={t("settings.snmp.help")}>
            <TextInput value={settings.snmpCommunity} onCommit={(v) => save({ snmpCommunity: v })} width={180} />
          </Field>
        </div>
      </Panel>

      <Panel>
        <div className="stack">
          <Field label={t("settings.report")}>
            <div className="stack" style={{ gap: 8 }}>
              <label className="row" style={{ gap: 8 }}>
                <span className="muted small" style={{ width: 90 }}>
                  {t("settings.report.company")}
                </span>
                <TextInput value={settings.reportCompany} onCommit={(v) => save({ reportCompany: v })} />
              </label>
              <label className="row" style={{ gap: 8 }}>
                <span className="muted small" style={{ width: 90 }}>
                  {t("settings.report.author")}
                </span>
                <TextInput value={settings.reportAuthor} onCommit={(v) => save({ reportAuthor: v })} />
              </label>
            </div>
          </Field>
          <Field label={t("settings.about")}>
            <div className="small">
              <div>{t("settings.about.version", { v: info?.version ?? "" })}</div>
              <div className="muted">
                {info?.os} {info?.arch}
              </div>
              <div className="muted" style={{ wordBreak: "break-all" }}>
                {t("settings.about.data")}: {info?.dataDir}
              </div>
            </div>
          </Field>
        </div>
      </Panel>
      <Toast message={toast} />
    </div>
  );
}
