import { useState } from "react";
import { Layers, Plus, X } from "lucide-react";
import { openFile } from "../../api";
import { useT } from "../../i18n";
import { addFloor, floorList, removeFloor, renameFloor, switchFloor, useSurvey } from "./surveyStore";

/** Tabs for the floors of a building; each floor has its own plan and data. */
export function FloorBar({ prepare, onError }: { prepare: (dataUrl: string) => Promise<{ url: string; w: number; h: number }>; onError: (msg: string) => void }) {
  const t = useT();
  const project = useSurvey((s) => s.project);
  const update = useSurvey((s) => s.update);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  if (!project) return null;
  const floors = floorList(project);

  const add = async () => {
    const f = await openFile("image/png,image/jpeg,image/svg+xml,image/webp", true);
    if (!f) return;
    try {
      const img = await prepare(f.content);
      const name = t("survey.floor.default", { n: floors.length + 1 });
      update((p) => addFloor(p, name, { name: f.name.replace(/\.[^.]+$/, ""), imageDataUrl: img.url, widthPx: img.w, heightPx: img.h, metersPerPx: null }));
      useSurvey.getState().set({ foundAps: [], selectedAp: null, walk: null });
    } catch {
      onError(t("survey.error.image"));
    }
  };
  const go = (id: string) => {
    update((p) => switchFloor(p, id));
    useSurvey.getState().set({ foundAps: [], selectedAp: null, walk: null });
  };
  const commit = () => {
    if (editing && draft.trim()) update((p) => renameFloor(p, editing, draft.trim()));
    setEditing(null);
  };

  return (
    <div className="floor-bar" role="tablist" aria-label={t("survey.floor.label")}>
      <Layers size={15} aria-hidden className="muted" />
      {floors.map((f) =>
        editing === f.id ? (
          <input
            key={f.id}
            className="input floor-input"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(null);
            }}
          />
        ) : (
          <span key={f.id} className="floor-tab" aria-selected={f.active} role="tab">
            <button
              onClick={() => (f.active ? (setEditing(f.id), setDraft(f.name)) : go(f.id))}
              title={f.active ? t("survey.floor.rename") : undefined}
            >
              {f.name}
              <span className="muted small"> {f.samples}</span>
            </button>
            {floors.length > 1 && f.active && (
              <button className="floor-x" aria-label={t("survey.floor.remove")} title={t("survey.floor.remove")} onClick={() => update((p) => removeFloor(p, f.id))}>
                <X size={12} />
              </button>
            )}
          </span>
        ),
      )}
      <button className="btn btn-ghost btn-sm" onClick={add}>
        <Plus size={14} />
        {t("survey.floor.add")}
      </button>
    </div>
  );
}
