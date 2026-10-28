import { create } from "zustand";
import { call } from "../../api";
import type { Band, Layer, Material, Sample, SurveyMode, SurveyProject, VirtualAp, Wall } from "../../types";

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
