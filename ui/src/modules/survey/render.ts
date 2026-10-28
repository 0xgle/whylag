import type { Heatmap, SurveyProject } from "../../types";
import { colorFor } from "./colors";

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = src;
  });
}

/**
 * Renders the floor plan with a heatmap layer, AP markers and sample points to a
 * PNG data URL. Used for exports and reports.
 */
export async function renderSurveyPng(
  project: SurveyProject,
  heatmap: Heatmap | null,
  opts: { opacity?: number; showSamples?: boolean; showAps?: boolean; maxWidth?: number } = {},
): Promise<string> {
  const { opacity = 0.68, showSamples = true, showAps = true, maxWidth = 1800 } = opts;
  const fp = project.floorPlan;
  const scale = Math.min(1, maxWidth / fp.widthPx) * (fp.widthPx < 1000 ? 2 : 1);
  const W = Math.round(fp.widthPx * scale);
  const H = Math.round(fp.heightPx * scale);
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, W, H);
  const plan = await loadImage(fp.imageDataUrl);
  ctx.drawImage(plan, 0, 0, W, H);

  if (heatmap) {
    const heat = document.createElement("canvas");
    heat.width = heatmap.cols;
    heat.height = heatmap.rows;
    const hctx = heat.getContext("2d")!;
    const img = hctx.createImageData(heatmap.cols, heatmap.rows);
    heatmap.values.forEach((v, i) => {
      if (v == null) return;
      const [r, g, b] = colorFor(heatmap.layer, v);
      img.data.set([r, g, b, 255], i * 4);
    });
    hctx.putImageData(img, 0, 0);
    ctx.save();
    ctx.globalAlpha = opacity;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(heat, 0, 0, heatmap.cols * heatmap.cellPx * scale, heatmap.rows * heatmap.cellPx * scale);
    ctx.restore();
  }

  if (showSamples && heatmap?.mode !== "predicted") {
    ctx.fillStyle = "rgba(23,32,43,0.55)";
    for (const s of project.samples) {
      ctx.beginPath();
      ctx.arc(s.x * scale, s.y * scale, 2.4 * Math.max(1, scale), 0, Math.PI * 2);
      ctx.fill();
    }
  }

  if (showAps) {
    const r = 9 * Math.max(1, scale * 0.8);
    ctx.font = `600 ${Math.round(12 * Math.max(1, scale * 0.8))}px Barlow, sans-serif`;
    ctx.textAlign = "center";
    for (const ap of project.aps) {
      const x = ap.x * scale;
      const y = ap.y * scale;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fillStyle = "#17202b";
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = "#ffffff";
      ctx.stroke();
      ctx.lineWidth = 4;
      ctx.strokeText(ap.name, x, y + r * 2.4);
      ctx.fillStyle = "#17202b";
      ctx.fillText(ap.name, x, y + r * 2.4);
    }
  }
  return canvas.toDataURL("image/png");
}
