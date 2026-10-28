import type { Diagnosis } from "../../types";

const GRADE_COLORS: Record<string, string> = {
  A: "#12805a",
  B: "#3fa34d",
  C: "#c99a12",
  D: "#d9632b",
  F: "#c0263d",
};

function wrap(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string[] {
  const words = text.split(" ");
  const lines: string[] = [];
  let line = "";
  for (const w of words) {
    const test = line ? `${line} ${w}` : w;
    if (ctx.measureText(test).width > maxWidth && line) {
      lines.push(line);
      line = w;
    } else {
      line = test;
    }
  }
  if (line) lines.push(line);
  return lines;
}

/**
 * Renders a 1200×630 share image (the size social networks preview) with the
 * grade, the verdict and the key numbers.
 */
export async function renderShareCard(
  d: Diagnosis,
  text: { title: string; verdict: string; footer: string; stats: { label: string; value: string }[] },
): Promise<string> {
  await document.fonts?.ready;
  const W = 1200;
  const H = 630;
  const c = document.createElement("canvas");
  c.width = W;
  c.height = H;
  const ctx = c.getContext("2d")!;
  ctx.fillStyle = "#edf0f3";
  ctx.fillRect(0, 0, W, H);
  // Faint measurement grid.
  ctx.strokeStyle = "rgba(23,32,43,0.05)";
  ctx.lineWidth = 1;
  for (let x = 0; x < W; x += 30) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, H);
    ctx.stroke();
  }
  for (let y = 0; y < H; y += 30) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(W, y);
    ctx.stroke();
  }

  // Grade tile.
  const color = GRADE_COLORS[d.grade] ?? "#2433c9";
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.roundRect(70, 120, 300, 300, 36);
  ctx.fill();
  ctx.fillStyle = "#ffffff";
  ctx.font = "700 230px 'Barlow Semi Condensed', Barlow, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(d.grade, 220, 282);

  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = "#4a5563";
  ctx.font = "600 30px Barlow, sans-serif";
  ctx.fillText(text.title, 420, 150);
  ctx.fillStyle = "#17202b";
  ctx.font = "600 56px 'Barlow Semi Condensed', Barlow, sans-serif";
  const lines = wrap(ctx, text.verdict, 710).slice(0, 3);
  lines.forEach((l, i) => ctx.fillText(l, 420, 220 + i * 62));

  // Stats row.
  const statsY = 220 + lines.length * 62 + 40;
  text.stats.slice(0, 3).forEach((s, i) => {
    const x = 420 + i * 240;
    ctx.fillStyle = "#7d8794";
    ctx.font = "500 24px Barlow, sans-serif";
    ctx.fillText(s.label, x, statsY);
    ctx.fillStyle = "#17202b";
    ctx.font = "700 40px 'Barlow Semi Condensed', Barlow, sans-serif";
    ctx.fillText(s.value, x, statsY + 46);
  });

  // Footer with the mark.
  ctx.fillStyle = "#2433c9";
  ctx.beginPath();
  ctx.roundRect(70, 520, 52, 52, 12);
  ctx.fill();
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 5;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(80, 552);
  ctx.bezierCurveTo(88, 530, 94, 530, 99, 552);
  ctx.bezierCurveTo(104, 574, 110, 574, 115, 548);
  ctx.stroke();
  ctx.fillStyle = "#17202b";
  ctx.font = "700 32px 'Barlow Semi Condensed', Barlow, sans-serif";
  ctx.fillText("WhyLag", 136, 557);
  ctx.fillStyle = "#7d8794";
  ctx.font = "500 24px Barlow, sans-serif";
  ctx.textAlign = "right";
  ctx.fillText(text.footer, W - 70, 557);
  return c.toDataURL("image/png");
}
