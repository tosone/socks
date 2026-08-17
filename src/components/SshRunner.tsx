import { useEffect, useMemo, useRef, useState, type UIEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { ChevronsDown, KeyRound, LockKeyhole, Play } from "lucide-react";
import { runSshSample } from "../api";
import type { SshAuthMode, SshRunEvent } from "../types";

const PRIVATE_KEY_PATH_KEY = "socks.ssh.privateKeyPath";
const DEFAULT_PRIVATE_KEY_PATH = "~/.ssh/id_ed25519";
const ANSI_PATTERN = /\x1b\[([0-9;]*)m/g;
const ANSI_COLORS: Record<number, string> = {
  30: "#555f6d",
  31: "#ff6b6b",
  32: "#4ade80",
  33: "#facc15",
  34: "#60a5fa",
  35: "#f472b6",
  36: "#22d3ee",
  37: "#e5e7eb",
  90: "#8b949e",
  91: "#f87171",
  92: "#86efac",
  93: "#fde047",
  94: "#93c5fd",
  95: "#f9a8d4",
  96: "#67e8f9",
  97: "#f8fafc",
};

type LogEntry = {
  id: number;
  stream: SshRunEvent["stream"];
  data: string;
};

type Segment = {
  text: string;
  color?: string;
  bold?: boolean;
  dim?: boolean;
};

export function SshRunner() {
  const [host, setHost] = useState("");
  const [port, setPort] = useState(22);
  const [username, setUsername] = useState("root");
  const [authMode, setAuthMode] = useState<SshAuthMode>("key");
  const [privateKeyPath, setPrivateKeyPath] = useState(DEFAULT_PRIVATE_KEY_PATH);
  const [password, setPassword] = useState("");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [follow, setFollow] = useState(true);
  const logRef = useRef<HTMLDivElement | null>(null);
  const runningRef = useRef(false);
  const nextIdRef = useRef(1);

  useEffect(() => {
    const savedPath = window.localStorage.getItem(PRIVATE_KEY_PATH_KEY);
    if (savedPath) {
      setPrivateKeyPath(savedPath);
    }
  }, []);

  useEffect(() => {
    if (authMode === "key") {
      window.localStorage.setItem(PRIVATE_KEY_PATH_KEY, privateKeyPath);
    }
  }, [authMode, privateKeyPath]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<SshRunEvent>("ssh-run", (event) => {
      const payload = normalizeTtyData(event.payload);
      setLogs((current) => [
        ...current,
        {
          id: nextIdRef.current++,
          stream: payload.stream,
          data: payload.data,
        },
      ]);
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!follow) {
      return;
    }
    scrollToBottom();
  }, [follow, logs]);

  const canRun = useMemo(() => {
    if (running) {
      return false;
    }
    if (host.trim().length === 0) {
      return false;
    }
    if (authMode === "key") {
      return privateKeyPath.trim().length > 0;
    }
    return password.length > 0;
  }, [authMode, host, password, privateKeyPath, running]);

  async function handleRun() {
    if (!canRun || runningRef.current) {
      return;
    }
    runningRef.current = true;
    setLogs([]);
    setFollow(true);
    setRunning(true);
    try {
      await runSshSample({
        host: host.trim(),
        port,
        username: username.trim(),
        authMode,
        privateKeyPath: authMode === "key" ? privateKeyPath.trim() : null,
        password: authMode === "password" ? password : null,
      });
    } catch (err) {
      pushLocalLog("stderr", `\x1b[31m${String(err)}\x1b[0m\n`);
    } finally {
      runningRef.current = false;
      setRunning(false);
    }
  }

  function pushLocalLog(stream: SshRunEvent["stream"], data: string) {
    setLogs((current) => [
      ...current,
      {
        id: nextIdRef.current++,
        stream,
        data,
      },
    ]);
  }

  function handleLogScroll(event: UIEvent<HTMLDivElement>) {
    const element = event.currentTarget;
    const distanceFromBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
    if (distanceFromBottom > 48) {
      setFollow(false);
    }
  }

  function enableFollow() {
    setFollow(true);
    window.requestAnimationFrame(scrollToBottom);
  }

  function scrollToBottom() {
    const element = logRef.current;
    if (!element) {
      return;
    }
    element.scrollTop = element.scrollHeight;
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col gap-4">
      <div className="rounded-xl border border-zinc-200 bg-white px-4 py-4 shadow-sm">
        <div className="grid gap-3 sm:grid-cols-[1fr_7rem_1fr]">
          <FieldText
            label="IP"
            value={host}
            placeholder="192.0.2.10"
            onChange={setHost}
          />
          <FieldNumber
            label="Port"
            value={port}
            onChange={setPort}
          />
          <FieldText
            label="User"
            value={username}
            placeholder="root"
            onChange={setUsername}
          />
        </div>

        <div className="mt-3 grid gap-3 sm:grid-cols-[1fr_auto]">
          {authMode === "key" ? (
            <FieldText
              label="Private key path"
              value={privateKeyPath}
              placeholder="~/.ssh/id_ed25519"
              onChange={setPrivateKeyPath}
            />
          ) : (
            <FieldPassword
              label="Password"
              value={password}
              onChange={setPassword}
            />
          )}
          <div className="grid grid-cols-[auto_auto] gap-2 self-end sm:grid-cols-1">
            <button
              type="button"
              className="inline-flex h-10 cursor-pointer items-center justify-center gap-2 rounded-lg border border-zinc-300 bg-white px-3 text-sm font-medium text-zinc-700 hover:bg-zinc-100"
              onClick={() => setAuthMode((current) => (current === "key" ? "password" : "key"))}
            >
              {authMode === "key" ? <LockKeyhole size={16} /> : <KeyRound size={16} />}
              {authMode === "key" ? "Use password" : "Use key"}
            </button>
            <button
              type="button"
              className="inline-flex h-10 cursor-pointer items-center justify-center gap-2 rounded-lg bg-zinc-900 px-4 text-sm font-medium text-white hover:bg-zinc-800 disabled:cursor-not-allowed disabled:opacity-50"
              onClick={handleRun}
              disabled={!canRun}
            >
              <Play size={16} fill="currentColor" />
              {running ? "Running" : "Run"}
            </button>
          </div>
        </div>
      </div>

      <div className="relative min-h-0 flex-1 overflow-hidden rounded-xl border border-zinc-800 bg-[#07090d] shadow-2xl shadow-zinc-950/15">
        <div className="absolute right-2 top-2 z-10">
          <button
            type="button"
            className={`inline-flex h-8 w-8 cursor-pointer items-center justify-center rounded-md border text-zinc-100 opacity-70 backdrop-blur transition hover:opacity-100 ${follow
              ? "border-emerald-400/25 bg-emerald-400/10"
              : "border-white/10 bg-white/5 hover:bg-white/10"
              }`}
            onClick={enableFollow}
            aria-label="Follow latest logs"
            title="Follow latest logs"
          >
            <ChevronsDown size={16} />
          </button>
        </div>
        <div className="h-full px-3 pb-2 pt-3">
          <div
            ref={logRef}
            className="h-full overflow-y-auto font-mono text-[12px] leading-5 text-zinc-100"
            onScroll={handleLogScroll}
          >
            {logs.length === 0 ? (
              <pre className="whitespace-pre-wrap text-zinc-500">
                {"> Waiting for execution.\n> Fill IP and authentication to run."}
              </pre>
            ) : (
              logs.map((log) => (
                <pre
                  key={log.id}
                  className={`whitespace-pre-wrap break-words ${log.stream === "stderr" ? "text-red-200" : ""}`}
                >
                  {renderAnsi(log.data)}
                </pre>
              ))
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function FieldText({
  label,
  value,
  placeholder,
  onChange,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-zinc-500">{label}</p>
      <input
        className="h-10 w-full rounded-lg border border-zinc-300 bg-white px-3 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}

function FieldPassword({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-zinc-500">{label}</p>
      <input
        className="h-10 w-full rounded-lg border border-zinc-300 bg-white px-3 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
        type="password"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}

function FieldNumber({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-zinc-500">{label}</p>
      <input
        className="h-10 w-full rounded-lg border border-zinc-300 bg-white px-3 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
        min={1}
        max={65535}
        type="number"
        value={value}
        onChange={(event) => onChange(clampPort(event.target.valueAsNumber))}
      />
    </div>
  );
}

function clampPort(value: number) {
  if (!Number.isFinite(value)) {
    return 22;
  }
  return Math.min(Math.max(Math.trunc(value), 1), 65535);
}

function normalizeTtyData(event: SshRunEvent): SshRunEvent {
  return {
    ...event,
    data: event.data.replace(/\r\n/g, "\n").replace(/\r/g, "\n"),
  };
}

function renderAnsi(value: string) {
  return parseAnsi(value).map((segment, index) => (
    <span
      key={`${index}-${segment.text}`}
      style={{
        color: segment.color,
        fontWeight: segment.bold ? 700 : undefined,
        opacity: segment.dim ? 0.72 : undefined,
      }}
    >
      {segment.text}
    </span>
  ));
}

function parseAnsi(value: string): Segment[] {
  const segments: Segment[] = [];
  let color: string | undefined;
  let bold = false;
  let dim = false;
  let index = 0;

  for (const match of value.matchAll(ANSI_PATTERN)) {
    if (match.index > index) {
      segments.push({ text: value.slice(index, match.index), color, bold, dim });
    }
    for (const code of parseAnsiCodes(match[1])) {
      if (code === 0) {
        color = undefined;
        bold = false;
        dim = false;
      } else if (code === 1) {
        bold = true;
      } else if (code === 2) {
        dim = true;
      } else if (code === 22) {
        bold = false;
        dim = false;
      } else if (code === 39) {
        color = undefined;
      } else if (ANSI_COLORS[code]) {
        color = ANSI_COLORS[code];
      }
    }
    index = match.index + match[0].length;
  }

  if (index < value.length) {
    segments.push({ text: value.slice(index), color, bold, dim });
  }
  return segments;
}

function parseAnsiCodes(raw: string) {
  if (!raw) {
    return [0];
  }
  return raw.split(";").map((code) => Number(code || 0));
}
