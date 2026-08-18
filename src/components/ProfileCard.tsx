import { useEffect, useRef, useState } from "react";
import { CircleAlert, CircleCheck, LoaderCircle, MoreHorizontal } from "lucide-react";
import type { ConnectivityStatus, Profile } from "../types";

type ProfileCardProps = {
  profile: Profile;
  connected: boolean;
  connecting: boolean;
  upBps: number;
  downBps: number;
  totalUpBytes: number;
  totalDownBytes: number;
  connectivityStatus?: ConnectivityStatus;
  menuPlacement?: "down" | "up";
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
};

export function ProfileCard({
  profile,
  connected,
  connecting,
  upBps,
  downBps,
  totalUpBytes,
  totalDownBytes,
  connectivityStatus,
  menuPlacement = "down",
  onToggle,
  onEdit,
  onDelete,
}: ProfileCardProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [showTotals, setShowTotals] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function onClick(event: MouseEvent) {
      if (!menuRef.current?.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  return (
    <article className="relative flex min-h-40 rounded-2xl border border-zinc-200 bg-white px-4 py-5 shadow-sm">
      <div className="relative z-10 flex flex-1 flex-col justify-between gap-4">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate text-lg font-semibold text-zinc-900">{profile.name}</h2>
            <ConnectivityIcon status={connectivityStatus} />
          </div>
          <div className="flex shrink-0 items-center gap-4">
            <button
              type="button"
              className="cursor-pointer rounded-lg px-2 py-1 text-right text-xs font-medium tabular-nums focus:outline-none"
              onClick={() => setShowTotals((current) => !current)}
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
            <div className="relative" ref={menuRef}>
              <button
                type="button"
                className="cursor-pointer rounded-lg p-1.5 text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800"
                onClick={() => setMenuOpen((open) => !open)}
                aria-label="More"
              >
                <MoreHorizontal size={18} className="pointer-events-none" />
              </button>
              {menuOpen ? (
                <div
                  className={`absolute right-0 z-20 w-32 cursor-pointer overflow-hidden rounded-lg border border-zinc-200 bg-white py-1 shadow-lg ${menuPlacement === "up" ? "bottom-full mb-1" : "top-full mt-1"
                    }`}
                >
                  <button
                    type="button"
                    className={`block w-full cursor-pointer px-3 py-1.5 text-left text-sm disabled:cursor-not-allowed disabled:opacity-60 ${connected ? "text-amber-700 hover:bg-amber-50" : "text-emerald-700 hover:bg-emerald-50"
                      }`}
                    onClick={() => {
                      setMenuOpen(false);
                      onToggle();
                    }}
                    disabled={connecting}
                  >
                    {connecting ? "Working..." : connected ? "Disconnect" : "Connect"}
                  </button>
                  <button
                    type="button"
                    className="block w-full cursor-pointer px-3 py-1.5 text-left text-sm text-zinc-700 hover:bg-zinc-50"
                    onClick={() => {
                      setMenuOpen(false);
                      onEdit();
                    }}
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    className="block w-full cursor-pointer px-3 py-1.5 text-left text-sm text-red-600 hover:bg-red-50"
                    onClick={() => {
                      setMenuOpen(false);
                      onDelete();
                    }}
                  >
                    Delete
                  </button>
                </div>
              ) : null}
            </div>
          </div>
        </div>
        <p className="truncate text-xs text-zinc-500">
          {profile.server}:{profile.port} · {profile.method}
        </p>
      </div>
    </article>
  );
}

function ConnectivityIcon({ status }: { status?: ConnectivityStatus }) {
  if (status === "checking") {
    return (
      <LoaderCircle
        size={16}
        className="shrink-0 animate-spin text-zinc-500"
        aria-label="Checking connection"
      />
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
