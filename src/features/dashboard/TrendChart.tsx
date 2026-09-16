import { useEffect, useRef, useState } from "react";

export interface TrendSeries {
  id: string;
  label: string;
  /** CSS color for the mark - always a `--chart-*` token, never a text token. */
  color: string;
  /** One value per shared x-slot; `null` where the series has no reading. */
  values: (number | null)[];
}

interface TrendChartProps {
  series: TrendSeries[];
  /** Shared x-axis timestamps (ms), oldest first. */
  timestamps: number[];
  /** Formats a value for axis ticks, direct labels, and the tooltip. */
  format: (value: number) => string;
  /** Lower bound for the y-domain's top, so a quiet chart doesn't
   * exaggerate noise by auto-scaling to a 2% range. */
  minTop: number;
  height?: number;
  /** How to label the x-axis.
   *
   * `"relative"` ("12m ago" … "now") suits the live poll, which is evenly
   * spaced and always ends at the present moment. Recorded history is
   * neither: its last sample may be days old and it has gaps wherever the
   * server was stopped, so `"absolute"` prints real timestamps instead of
   * claiming the right-hand edge is "now". */
  timeAxis?: "relative" | "absolute";
}

const PAD = { top: 12, right: 60, bottom: 20, left: 48 };

const DAY_MS = 24 * 60 * 60 * 1000;

/** Axis label for an absolute time: the date is only worth the space once
 * the window spans more than a day. */
function formatAxisTime(ms: number, spanMs: number): string {
  const d = new Date(ms);
  return spanMs > DAY_MS
    ? d.toLocaleDateString(undefined, { month: "short", day: "numeric" })
    : d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** Rounds a domain top up to a clean tick value (1/2/5 x 10^n). */
function niceTop(raw: number): number {
  if (raw <= 0) return 1;
  const exp = Math.floor(Math.log10(raw));
  const mag = Math.pow(10, exp);
  const norm = raw / mag;
  const step = norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10;
  return step * mag;
}

/**
 * Multi-series line chart, rendered as inline SVG at real pixel size (via
 * ResizeObserver) rather than a scaled viewBox - a stretched viewBox would
 * distort the fixed 2px stroke and the marker rings along with it.
 *
 * Deliberately dependency-free: a charting library would be a large add to
 * an already-large bundle, and the app's CSP forbids loading one from a CDN.
 */
export function TrendChart({
  series,
  timestamps,
  format,
  minTop,
  height = 160,
  timeAxis = "relative",
}: TrendChartProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      setWidth(entry.contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const plotW = Math.max(0, width - PAD.left - PAD.right);
  const plotH = Math.max(0, height - PAD.top - PAD.bottom);

  const maxValue = Math.max(
    minTop,
    ...series.flatMap((s) => s.values.filter((v): v is number => v !== null)),
  );
  const top = niceTop(maxValue);

  const n = timestamps.length;
  // Positioned by actual time, not by index. With the evenly spaced live
  // poll the two are identical, but recorded history is sparse and gappy -
  // index positioning would silently close the gap where a server was
  // stopped for two days and draw a straight line across it.
  const tStart = n > 0 ? timestamps[0] : 0;
  const tEnd = n > 0 ? timestamps[n - 1] : 0;
  const tSpan = tEnd - tStart;
  const x = (i: number) =>
    n <= 1 || tSpan <= 0 ? plotW : ((timestamps[i] - tStart) / tSpan) * plotW;
  const y = (v: number) => plotH - (v / top) * plotH;

  const ticks = [0, top / 2, top];

  function handlePointer(e: React.PointerEvent<SVGSVGElement>) {
    if (n === 0 || plotW <= 0) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - rect.left - PAD.left;
    const ratio = Math.min(1, Math.max(0, px / plotW));
    if (tSpan <= 0) {
      setHoverIndex(n - 1);
      return;
    }
    // Nearest sample by time, matching how points are placed above.
    const targetT = tStart + ratio * tSpan;
    let nearest = 0;
    for (let i = 1; i < n; i++) {
      if (Math.abs(timestamps[i] - targetT) < Math.abs(timestamps[nearest] - targetT)) {
        nearest = i;
      }
    }
    setHoverIndex(nearest);
  }

  if (width === 0) {
    // First paint before measurement - reserve the space so the card
    // doesn't jump once the real size is known.
    return <div ref={wrapRef} style={{ height }} />;
  }

  const hoverX = hoverIndex !== null ? PAD.left + x(hoverIndex) : 0;

  // Keep only end-labels that clear their neighbours vertically; the rest
  // are dropped (never stacked - see the note at the render site).
  const MIN_LABEL_GAP = 12;
  const endLabels = series
    .map((s) => {
      const idx = s.values.reduce<number>((acc, v, i) => (v !== null ? i : acc), -1);
      const value = idx < 0 ? null : s.values[idx];
      return value === null
        ? null
        : { id: s.id, x: PAD.left + x(idx) + 8, y: PAD.top + y(value), text: format(value) };
    })
    .filter((l): l is { id: string; x: number; y: number; text: string } => l !== null)
    .sort((a, b) => a.y - b.y)
    // Greedy against the last *kept* label, not the last sorted one - the
    // previous entry may itself have been dropped.
    .reduce<{ id: string; x: number; y: number; text: string }[]>((kept, label) => {
      const last = kept[kept.length - 1];
      if (!last || label.y - last.y >= MIN_LABEL_GAP) kept.push(label);
      return kept;
    }, []);

  return (
    <div ref={wrapRef} className="relative w-full">
      <svg
        width={width}
        height={height}
        role="img"
        aria-label={`Trend chart: ${series.map((s) => s.label).join(", ")}`}
        onPointerMove={handlePointer}
        onPointerLeave={() => setHoverIndex(null)}
      >
        <g transform={`translate(${PAD.left},${PAD.top})`}>
          {/* Recessive hairline gridlines + y ticks. */}
          {ticks.map((t) => (
            <g key={t}>
              <line
                x1={0}
                x2={plotW}
                y1={y(t)}
                y2={y(t)}
                className="stroke-border"
                strokeWidth={1}
              />
              <text
                x={-8}
                y={y(t)}
                dy="0.32em"
                textAnchor="end"
                className="fill-muted-foreground text-[10px]"
              >
                {format(t)}
              </text>
            </g>
          ))}

          {series.map((s) => {
            const points = s.values
              .map((v, i) => (v === null ? null : ([x(i), y(v)] as const)))
              .filter((p): p is readonly [number, number] => p !== null);
            if (points.length === 0) return null;
            const d = points.map((p, i) => `${i === 0 ? "M" : "L"}${p[0]},${p[1]}`).join(" ");
            const last = points[points.length - 1];
            return (
              <g key={s.id}>
                <path
                  d={d}
                  fill="none"
                  stroke={s.color}
                  strokeWidth={2}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
                {/* End marker: >=8px, with a 2px surface ring so it stays
                    legible where lines cross. */}
                <circle
                  cx={last[0]}
                  cy={last[1]}
                  r={4}
                  fill={s.color}
                  className="stroke-card"
                  strokeWidth={2}
                />
              </g>
            );
          })}

          {/* Crosshair finds the X - readers aim at a moment, not a 2px line. */}
          {hoverIndex !== null && (
            <line
              x1={x(hoverIndex)}
              x2={x(hoverIndex)}
              y1={0}
              y2={plotH}
              className="stroke-muted-foreground"
              strokeWidth={1}
            />
          )}
        </g>

        {/* Direct end-labels, collision-aware: when lines converge, stacking
            or nudging labels detaches them from their line and reads as
            noise, so a colliding label is dropped rather than moved - the
            legend, tooltip, and table view still carry its value. */}
        {endLabels.map((label) => (
          <text
            key={label.id}
            x={label.x}
            y={label.y}
            dy="0.32em"
            className="fill-foreground text-[10px] font-medium"
          >
            {label.text}
          </text>
        ))}

        <text
          x={PAD.left}
          y={height - 4}
          className="fill-muted-foreground text-[10px]"
        >
          {timeAxis === "absolute"
            ? formatAxisTime(timestamps[0], tSpan)
            : `${Math.round((tSpan / 1000 / 60) * 10) / 10 || 0}m ago`}
        </text>
        <text
          x={PAD.left + plotW}
          y={height - 4}
          textAnchor="end"
          className="fill-muted-foreground text-[10px]"
        >
          {timeAxis === "absolute" ? formatAxisTime(timestamps[n - 1], tSpan) : "now"}
        </text>
      </svg>

      {hoverIndex !== null && (
        <div
          className="pointer-events-none absolute top-2 z-10 min-w-32 rounded-lg border border-border bg-popover p-2 shadow-md"
          style={{
            left: Math.min(Math.max(hoverX + 8, 0), Math.max(0, width - 150)),
          }}
        >
          <p className="mb-1 text-[10px] text-muted-foreground">
            {tSpan > DAY_MS
              ? new Date(timestamps[hoverIndex]).toLocaleString()
              : new Date(timestamps[hoverIndex]).toLocaleTimeString()}
          </p>
          {/* One tooltip, every series - the pointer never has to land on a
              line to get a value. Values lead, labels follow. */}
          {series.map((s) => (
            <div key={s.id} className="flex items-center gap-1.5 text-[11px]">
              <span
                className="inline-block h-0.5 w-3 shrink-0 rounded-full"
                style={{ backgroundColor: s.color }}
              />
              <span className="font-medium text-foreground">
                {s.values[hoverIndex] === null ? "—" : format(s.values[hoverIndex] as number)}
              </span>
              <span className="truncate text-muted-foreground">{s.label}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
