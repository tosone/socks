import { useId, useLayoutEffect, useRef, useState } from "react";
import { CircleAlert, CircleCheck, Pencil, Trash2 } from "lucide-react";
import type { ConnectivityStatus, Profile } from "../types";

const CORNER_RADIUS = 16; // rounded-2xl
// Cyberpunk-ish neon: light green bleeding into violet and fuchsia.
const BORDER_GRADIENT_STOPS = [
  { offset: "0%", color: "#6ee7b7" }, // emerald-300
  { offset: "50%", color: "#a78bfa" }, // violet-400
  { offset: "100%", color: "#e879f9" }, // fuchsia-400
];
const BORDER_GLOW =
  "drop-shadow(0 0 2px rgba(110, 231, 183, 0.85)) drop-shadow(0 0 6px rgba(168, 85, 247, 0.6)) drop-shadow(0 0 14px rgba(232, 121, 249, 0.35))";
const HALF_DURATION = 520; // top/bottom edges draw from the center outwards
const SIDE_DURATION = 420; // then the left/right edges wrap around
const SIDE_DELAY = 460;

/** Rounded-rectangle outline split into "halves" (top/bottom, drawn from the
 *  center) and "sides" (left/right, drawn afterwards to close the border). */
function borderGeometry(width: number, height: number) {
  const r = Math.min(CORNER_RADIUS, width / 2, height / 2);
  const halves = [
    `M ${width / 2} 0 L ${width - r} 0 A ${r} ${r} 0 0 1 ${width} ${r}`,
    `M ${width / 2} 0 L ${r} 0 A ${r} ${r} 0 0 0 0 ${r}`,
    `M ${width / 2} ${height} L ${width - r} ${height} A ${r} ${r} 0 0 0 ${width} ${height - r}`,
    `M ${width / 2} ${height} L ${r} ${height} A ${r} ${r} 0 0 1 0 ${height - r}`,
  ];
  // Split each vertical side so both ends draw towards the middle and finish
  // there, instead of running straight from top to bottom.
  const middle = height / 2;
  const sides = [
    `M 0 ${r} L 0 ${middle}`,
    `M 0 ${height - r} L 0 ${middle}`,
    `M ${width} ${r} L ${width} ${middle}`,
    `M ${width} ${height - r} L ${width} ${middle}`,
  ];
  return {
    halves,
    sides,
    halfLength: Math.max(0, width / 2 - r) + (Math.PI * r) / 2,
    sideLength: Math.max(0, middle - r),
  };
}

type ProfileCardProps = {
  profile: Profile;
  connecting: boolean;
  connected: boolean;
  upBps: number;
  downBps: number;
  totalUpBytes: number;
  totalDownBytes: number;
  connectivityStatus?: ConnectivityStatus;
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
};

export function ProfileCard({
  profile,
  connecting,
  connected,
  upBps,
  downBps,
  totalUpBytes,
  totalDownBytes,
  connectivityStatus,
  onToggle,
  onEdit,
  onDelete,
}: ProfileCardProps) {
  const [showTotals, setShowTotals] = useState(false);
  const revealed = connected || connecting;
  const gradientId = `profile-border-${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const articleRef = useRef<HTMLElement | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const node = articleRef.current;
    if (!node) {
      return;
    }
    const measure = () => {
      const width = node.offsetWidth;
      const height = node.offsetHeight;
      setSize((current) =>
        current.width === width && current.height === height ? current : { width, height },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const border =
    size.width > 0 && size.height > 0 ? borderGeometry(size.width, size.height) : null;

  return (
    <article
      ref={articleRef}
      className="relative flex min-h-40 cursor-pointer rounded-2xl bg-white px-4 pb-2.5 pt-5 shadow-sm"
      style={{
        boxShadow: revealed
          ? "0 0 0 1px rgba(167, 139, 250, 0.35), 0 12px 32px -12px rgba(110, 231, 183, 0.55), 0 8px 28px -10px rgba(168, 85, 247, 0.5)"
          : undefined,
        transition: "box-shadow 700ms ease-out",
      }}
      onDoubleClick={() => {
        if (!connecting) {
          onToggle();
        }
      }}
    >
      {border ? (
        <svg
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 overflow-visible"
          width={size.width}
          height={size.height}
          viewBox={`0 0 ${size.width} ${size.height}`}
          fill="none"
        >
          <defs>
            <linearGradient
              id={gradientId}
              gradientUnits="userSpaceOnUse"
              x1={0}
              y1={0}
              x2={size.width}
              y2={size.height}
            >
              {BORDER_GRADIENT_STOPS.map((stop) => (
                <stop key={stop.offset} offset={stop.offset} stopColor={stop.color} />
              ))}
            </linearGradient>
          </defs>
          <rect
            x={0.5}
            y={0.5}
            width={size.width - 1}
            height={size.height - 1}
            rx={CORNER_RADIUS - 0.5}
            stroke="#e4e4e7"
            strokeWidth={1}
          />
          <g
            style={{
              filter: revealed ? BORDER_GLOW : undefined,
              transition: "filter 500ms ease-out",
            }}
          >
            {border.halves.map((d, index) => (
              <path
                key={`half-${index}`}
                d={d}
                stroke={`url(#${gradientId})`}
                strokeWidth={2}
                strokeLinecap="round"
                style={{
                  strokeDasharray: border.halfLength,
                  strokeDashoffset: revealed ? 0 : border.halfLength,
                  transition: `stroke-dashoffset ${HALF_DURATION}ms ease-out ${
                    revealed ? 0 : SIDE_DURATION
                  }ms`,
                }}
              />
            ))}
            {border.sides.map((d, index) => (
              <path
                key={`side-${index}`}
                d={d}
                stroke={`url(#${gradientId})`}
                strokeWidth={2}
                strokeLinecap="round"
                style={{
                  strokeDasharray: border.sideLength,
                  strokeDashoffset: revealed ? 0 : border.sideLength,
                  transition: `stroke-dashoffset ${SIDE_DURATION}ms ease-out ${
                    revealed ? SIDE_DELAY : 0
                  }ms`,
                }}
              />
            ))}
          </g>
        </svg>
      ) : null}
      <div className="relative z-10 flex flex-1 flex-col justify-between gap-4">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate text-lg font-semibold text-zinc-900">{profile.name}</h2>
            <ConnectivityIcon status={connectivityStatus} />
          </div>
          <button
            type="button"
            className="shrink-0 cursor-pointer rounded-lg px-2 py-1 text-right text-xs font-medium tabular-nums focus:outline-none"
            onClick={(event) => {
              event.stopPropagation();
              setShowTotals((current) => !current);
            }}
            onDoubleClick={(event) => event.stopPropagation()}
            aria-label={showTotals ? "Show current speed" : "Show total traffic"}
            title={showTotals ? "Show current speed" : "Show total traffic"}
          >
            <div
              key={showTotals ? "total-up" : "speed-up"}
              className="text-blue-600 transition-opacity duration-150"
            >
              ↑ {showTotals ? formatBytes(totalUpBytes) : formatSpeed(upBps)}
            </div>
            <div
              key={showTotals ? "total-down" : "speed-down"}
              className="mt-1 text-emerald-600 transition-opacity duration-150"
            >
              ↓ {showTotals ? formatBytes(totalDownBytes) : formatSpeed(downBps)}
            </div>
          </button>
        </div>
        <div className="flex items-center justify-between gap-3">
          <p className="min-w-0 truncate text-xs text-zinc-500">
            {profile.server}:{profile.port}
          </p>
          <div className="flex shrink-0 items-center gap-0.5">
            <button
              type="button"
              className="inline-flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800"
              onClick={(event) => {
                event.stopPropagation();
                onEdit();
              }}
              onDoubleClick={(event) => event.stopPropagation()}
              aria-label="Edit"
              title="Edit"
            >
              <Pencil size={14} />
            </button>
            <button
              type="button"
              className="inline-flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-zinc-500 hover:bg-red-50 hover:text-red-600"
              onClick={(event) => {
                event.stopPropagation();
                onDelete();
              }}
              onDoubleClick={(event) => event.stopPropagation()}
              aria-label="Delete"
              title="Delete"
            >
              <Trash2 size={14} />
            </button>
          </div>
        </div>
      </div>
    </article>
  );
}

function ConnectivityIcon({ status }: { status?: ConnectivityStatus }) {
  if (status === "checking") {
    return (
      <span
        className="inline-flex h-4 w-4 shrink-0 items-center justify-center"
        aria-label="Checking connection"
      >
        <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-zinc-300 border-t-zinc-600" />
      </span>
    );
  }
  if (status === "connected") {
    return (
      <CircleCheck
        size={16}
        className="shrink-0 text-emerald-600"
        aria-label="Connection verified"
      />
    );
  }
  if (status === "failed") {
    return (
      <CircleAlert
        size={16}
        className="shrink-0 text-red-600"
        aria-label="Connection check failed"
      />
    );
  }
  return null;
}

function formatSpeed(bps: number) {
  if (bps < 1024) {
    return `${bps.toFixed(0)} B/s`;
  }
  if (bps < 1024 * 1024) {
    return `${(bps / 1024).toFixed(1)} KB/s`;
  }
  return `${(bps / 1024 / 1024).toFixed(1)} MB/s`;
}

function formatBytes(bytes: number) {
  if (bytes < 1024) {
    return `${bytes.toFixed(0)} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}
