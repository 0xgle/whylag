import { useT } from "../i18n";

export function Placeholder() {
  const t = useT();
  return <div className="empty">{t("placeholder.soon")}</div>;
}
