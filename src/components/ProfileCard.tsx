import { useState } from "react";
import { CircleAlert, CircleCheck, Pencil, Trash2 } from "lucide-react";
import type { ConnectivityStatus, Profile } from "../types";

type ProfileCardProps = {
  profile: Profile;
  connecting: boolean;
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

  return (
    <article
      className="relative flex min-h-40 cursor-pointer rounded-2xl border border-zinc-200 bg-white px-4 pb-2.5 pt-5 shadow-sm"
      onDoubleClick={() => {
        if (!connecting) {
          onToggle();
        }
      }}
    >
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
