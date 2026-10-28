import { create } from "zustand";
import { call } from "../../api";
import type { Band, Floor, FloorPlanInfo, Layer, LocatedAp, Material, Sample, SurveyMode, SurveyProject, VirtualAp, Wall } from "../../types";

export type Tool = "measure" | "walk" | "calibrate" | "wall" | "ap" | "select";

export interface WalkState {
  /** Waypoints clicked so far, with the time they were clicked. */
  waypoints: { x: number; y: number; t: number }[];
  /** Scans collected since the walk started, waiting for positions. */
  pending: { t: number; readings: Sample["readings"] }[];
}

interface SurveyState {
  project: SurveyProject | null;
  undoStack: SurveyProject[];
  redoStack: SurveyProject[];

  tool: Tool;
  mode: SurveyMode;
  layer: Layer;
  ssids: string[];
  bands: Band[];
  opacity: number;
  wallMaterial: Material;
  selectedAp: string | null;
  walk: WalkState | null;
  calibration: { a: { x: number; y: number } | null; b: { x: number; y: number } | null };
  /** Run a short throughput test at each point measurement. */
  measureThroughput: boolean;
  throughputHost: string;
  throughputCode: string;
  throughputKind: "peer" | "iperf3";
  /** Show access points located from the measurements. */
  showFoundAps: boolean;
  foundAps: LocatedAp[];
  /** Mark where the computer switched access points along the walk. */
  showRoams: boolean;

  setProject: (p: SurveyProject | null, recordUndo?: boolean) => void;
  update: (fn: (p: SurveyProject) => SurveyProject) => void;
  undo: () => void;
  redo: () => void;
  set: (patch: Partial<Omit<SurveyState, "setProject" | "update" | "undo" | "redo" | "set">>) => void;
}

const UNDO_LIMIT = 60;

let autosaveTimer: ReturnType<typeof setTimeout> | undefined;
function scheduleAutosave(p: SurveyProject | null) {
  clearTimeout(autosaveTimer);
  if (!p) return;
  autosaveTimer = setTimeout(() => {
    call("project.autosave", { content: JSON.stringify(p) }).catch(() => {});
  }, 800);
}

export const useSurvey = create<SurveyState>((set, get) => ({
  project: null,
  undoStack: [],
  redoStack: [],
  tool: "measure",
  mode: "measured",
  layer: "signal",
  ssids: [],
  bands: [],
  opacity: 0.68,
  wallMaterial: "drywall",
  selectedAp: null,
  walk: null,
  calibration: { a: null, b: null },
  measureThroughput: false,
  throughputHost: "",
  throughputCode: "",
  throughputKind: "peer",
  showFoundAps: false,
  foundAps: [],
  showRoams: true,

  setProject: (project, recordUndo = false) => {
    const prev = get().project;
    set({
      project,
      undoStack: recordUndo && prev ? [...get().undoStack, prev].slice(-UNDO_LIMIT) : recordUndo ? get().undoStack : [],
      redoStack: [],
    });
    scheduleAutosave(project);
  },
  update: (fn) => {
    const prev = get().project;
    if (!prev) return;
    const next = fn(prev);
    set({ project: next, undoStack: [...get().undoStack, prev].slice(-UNDO_LIMIT), redoStack: [] });
    scheduleAutosave(next);
  },
  undo: () => {
    const { undoStack, project } = get();
    if (!undoStack.length || !project) return;
    const prev = undoStack[undoStack.length - 1];
    set({ project: prev, undoStack: undoStack.slice(0, -1), redoStack: [...get().redoStack, project] });
    scheduleAutosave(prev);
  },
  redo: () => {
    const { redoStack, project } = get();
    if (!redoStack.length || !project) return;
    const next = redoStack[redoStack.length - 1];
    set({ project: next, redoStack: redoStack.slice(0, -1), undoStack: [...get().undoStack, project] });
    scheduleAutosave(next);
  },
  set: (patch) => set(patch),
}));

export function newProject(name: string, imageDataUrl: string, widthPx: number, heightPx: number, metersPerPx: number | null, walls: Wall[] = [], aps: VirtualAp[] = []): SurveyProject {
  return {
    version: 1,
    name,
    createdMs: Date.now(),
    floorPlan: { name, imageDataUrl, widthPx, heightPx, metersPerPx },
    samples: [],
    walls,
    aps,
  };
}

export function isProject(x: unknown): x is SurveyProject {
  const p = x as SurveyProject;
  return !!p && typeof p === "object" && p.version === 1 && !!p.floorPlan?.imageDataUrl && Array.isArray(p.samples);
}

// ----- Floors -----
// The active floor's data stays on the project's top-level fields, so every
// tool keeps working unchanged; other floors wait in `floors`.

export function floorId(p: SurveyProject) {
  return p.floorId ?? "floor-1";
}

export function floorList(p: SurveyProject): { id: string; name: string; active: boolean; samples: number }[] {
  const active = { id: floorId(p), name: p.floorName ?? p.floorPlan.name, active: true, samples: p.samples.length };
  const others = (p.floors ?? []).map((f) => ({ id: f.id, name: f.name, active: false, samples: f.samples.length }));
  return [active, ...others].sort((a, b) => floorOrder(p, a.id) - floorOrder(p, b.id));
}

/** Floors keep the order they were added in (ids are floor-1, floor-2...). */
function floorOrder(_p: SurveyProject, id: string) {
  const n = parseInt(id.replace(/\D+/g, ""), 10);
  return Number.isFinite(n) ? n : 0;
}

function activeAsFloor(p: SurveyProject): Floor {
  return { id: floorId(p), name: p.floorName ?? p.floorPlan.name, floorPlan: p.floorPlan, samples: p.samples, walls: p.walls, aps: p.aps };
}

export function switchFloor(p: SurveyProject, id: string): SurveyProject {
  if (id === floorId(p)) return p;
  const target = (p.floors ?? []).find((f) => f.id === id);
  if (!target) return p;
  return {
    ...p,
    floorId: target.id,
    floorName: target.name,
    floorPlan: target.floorPlan,
    samples: target.samples,
    walls: target.walls,
    aps: target.aps,
    floors: [...(p.floors ?? []).filter((f) => f.id !== id), activeAsFloor(p)],
  };
}

export function addFloor(p: SurveyProject, name: string, plan: FloorPlanInfo): SurveyProject {
  const ids = floorList(p).map((f) => floorOrder(p, f.id));
  const id = `floor-${Math.max(0, ...ids) + 1}`;
  return {
    ...p,
    floorId: id,
    floorName: name,
    // A new plan of the same building usually has the same scale.
    floorPlan: { ...plan, metersPerPx: plan.metersPerPx ?? (plan.widthPx === p.floorPlan.widthPx ? p.floorPlan.metersPerPx : null) },
    samples: [],
    walls: [],
    aps: [],
    floors: [...(p.floors ?? []), activeAsFloor(p)],
  };
}

export function renameFloor(p: SurveyProject, id: string, name: string): SurveyProject {
  if (id === floorId(p)) return { ...p, floorName: name };
  return { ...p, floors: (p.floors ?? []).map((f) => (f.id === id ? { ...f, name } : f)) };
}

/** Removes a floor; removing the active one switches to another first. */
export function removeFloor(p: SurveyProject, id: string): SurveyProject {
  const others = p.floors ?? [];
  if (!others.length) return p;
  if (id !== floorId(p)) return { ...p, floors: others.filter((f) => f.id !== id) };
  const next = switchFloor(p, others[0].id);
  return { ...next, floors: (next.floors ?? []).filter((f) => f.id !== id) };
}

/** Every floor as a standalone project (for reports). */
export function allFloors(p: SurveyProject): SurveyProject[] {
  const list = [p, ...(p.floors ?? []).map((f) => switchFloor(p, f.id))];
  return list.sort((a, b) => floorOrder(p, floorId(a)) - floorOrder(p, floorId(b)));
}
