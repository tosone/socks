import type { SpeedSample } from "../types";

type SparklineProps = {
  samples: SpeedSample[];
  padding?: {
    top: number;
    right: number;
    bottom: number;
    left: number;
  };
};

function toPoints(
  values: number[],
  width: number,
  height: number,
  padding: NonNullable<SparklineProps["padding"]>,
) {
  if (values.length === 0) {
    return "";
  }
  const chartWidth = width - padding.left - padding.right;
  const chartHeight = height - padding.top - padding.bottom;
  const max = Math.max(...values, 1);
  return values
    .map((value, index) => {
      const x = padding.left + (values.length === 1 ? 0 : (index / (values.length - 1)) * chartWidth);
      const y = padding.top + chartHeight - (value / max) * chartHeight;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

export function Sparkline({
  samples,
  padding = { top: 28, right: 40, bottom: 24, left: 118 },
}: SparklineProps) {
  const width = 320;
  const height = 96;
  const up = samples.map((sample) => sample.up);
  const down = samples.map((sample) => sample.down);

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className="pointer-events-none absolute inset-0 h-full w-full opacity-30"
      preserveAspectRatio="none"
    >
      <polyline
        fill="none"
        stroke="#2563eb"
        strokeWidth="2"
        points={toPoints(up, width, height, padding)}
      />
      <polyline
        fill="none"
        stroke="#059669"
        strokeWidth="2"
        points={toPoints(down, width, height, padding)}
      />
    </svg>
  );
}
