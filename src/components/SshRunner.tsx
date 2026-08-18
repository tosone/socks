import { useEffect, useMemo, useRef, useState, type UIEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  ChevronDown,
  ChevronsDown,
  Eye,
  EyeOff,
  Play,
  Server,
} from "lucide-react";
import { runSshSample } from "../api";
import type { ProfileInput, SshAuthMode, SshRunEvent } from "../types";

const PRIVATE_KEY_PATH_KEY = "socks.ssh.privateKeyPath";
const DEFAULT_PRIVATE_KEY_PATH = "~/.ssh/id_ed25519";
const DEFAULT_METHOD = "2022-blake3-chacha20-poly1305";
const inputBaseClass =
  "h-10 w-full rounded-lg border border-zinc-300 bg-white text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200";
const inputClass = `${inputBaseClass} px-3`;
const passwordInputClass = `${inputBaseClass} py-0 pl-3 pr-10`;
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

type SshRunnerProps = {
  ciphers: string[];
  onAddProfile: (input: ProfileInput) => Promise<void>;
};

export function SshRunner({ ciphers, onAddProfile }: SshRunnerProps) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState(22);
  const [username, setUsername] = useState("root");
  const [authMode, setAuthMode] = useState<SshAuthMode>("key");
  const [privateKeyPath, setPrivateKeyPath] = useState(DEFAULT_PRIVATE_KEY_PATH);
  const [password, setPassword] = useState("");
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [servicePassword, setServicePassword] = useState("change-me");
  const [servicePasswordVisible, setServicePasswordVisible] = useState(false);
  const [method, setMethod] = useState(ciphers[0] ?? DEFAULT_METHOD);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [installedProfile, setInstalledProfile] = useState<ProfileInput | null>(null);
  const [addingProfile, setAddingProfile] = useState(false);
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
    if (!ciphers.includes(method)) {
      setMethod(ciphers[0] ?? DEFAULT_METHOD);
    }
  }, [ciphers, method]);

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
    if (servicePassword.length === 0 || method.trim().length === 0) {
      return false;
    }
    if (authMode === "key") {
      return privateKeyPath.trim().length > 0;
    }
    return password.length > 0;
  }, [authMode, host, method, password, privateKeyPath, running, servicePassword]);

  async function handleRun() {
    if (!canRun || runningRef.current) {
      return;
    }
    runningRef.current = true;
    setLogs([]);
    setFollow(true);
    setInstalledProfile(null);
    setRunning(true);
    try {
      const result = await runSshSample({
        host: host.trim(),
        port,
        username: username.trim(),
        authMode,
        privateKeyPath: authMode === "key" ? privateKeyPath.trim() : null,
        password: authMode === "password" ? password : null,
        servicePassword,
        method,
      });
      if (result.exitStatus === 0) {
        setInstalledProfile(buildProfileInput());
      }
    } catch (err) {
      pushLocalLog("stderr", `\x1b[31m${String(err)}\x1b[0m\n`);
    } finally {
      runningRef.current = false;
      setRunning(false);
    }
  }

  async function handleAddProfile() {
    if (!installedProfile || addingProfile) {
      return;
    }
    setAddingProfile(true);
    try {
      await onAddProfile(installedProfile);
      pushLocalLog("system", "\x1b[32mServer profile added.\x1b[0m\n");
      setInstalledProfile(null);
    } catch (err) {
      pushLocalLog("stderr", `\x1b[31m${String(err)}\x1b[0m\n`);
    } finally {
      setAddingProfile(false);
    }
  }

  function buildProfileInput(): ProfileInput {
    const server = host.trim();
    return {
      name: server.slice(0, 10) || "server",
      server,
      port: 443,
      password: servicePassword,
      method,
      plugin: "v2ray-plugin",
      pluginOpts: `tls;host=${server}`,
    };
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
    <section className="flex min-h-full flex-1 flex-col gap-3">
      <div className="rounded-xl border border-zinc-200 bg-white px-4 py-4 shadow-sm">
        <div>
          <div>
            <p className="mb-1.5 text-xs font-medium text-zinc-500">IP / Port</p>
            <div className="grid grid-cols-[1fr_5rem] gap-2">
              <input
                className={inputClass}
                value={host}
                placeholder="192.0.2.10"
                onChange={(event) => setHost(event.target.value)}
              />
              <input
                className={`${inputClass} appearance-none [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none`}
                min={1}
                max={65535}
                type="number"
                value={port}
                onChange={(event) => setPort(clampPort(event.target.valueAsNumber))}
              />
            </div>
          </div>
        </div>
        <div className="mt-3">
          <FieldText
            label="User"
            value={username}
            placeholder="root"
            onChange={setUsername}
          />
        </div>

        <div className="mt-3">
          <p className="mb-1.5 text-xs font-medium text-zinc-500">
            {authMode === "key" ? "Private key path" : "SSH password"}
          </p>
          <div className="grid grid-cols-[1fr_6.5rem] gap-2">
            {authMode === "key" ? (
              <input
                className={inputClass}
                value={privateKeyPath}
                placeholder="~/.ssh/id_ed25519"
                onChange={(event) => setPrivateKeyPath(event.target.value)}
              />
            ) : (
              <PasswordInput
                value={password}
                visible={passwordVisible}
                onChange={setPassword}
                onToggle={() => setPasswordVisible((current) => !current)}
              />
            )}
            <button
              type="button"
              className="inline-flex h-10 w-full cursor-pointer items-center justify-center gap-2 rounded-lg border border-zinc-300 bg-white px-3 text-sm font-medium text-zinc-700 hover:bg-zinc-100"
              onClick={() => setAuthMode((current) => (current === "key" ? "password" : "key"))}
            >
              {authMode === "key" ? "Password" : "Key"}
            </button>
          </div>
        </div>

        <div className="mt-3 grid grid-cols-[1fr_11rem] gap-3">
          <FieldSelect
            label="Encryption"
            value={method}
            options={ciphers.length > 0 ? ciphers : [DEFAULT_METHOD]}
            onChange={setMethod}
          />
          <FieldPassword
            label="Server password"
            value={servicePassword}
            visible={servicePasswordVisible}
            onChange={setServicePassword}
            onToggle={() => setServicePasswordVisible((current) => !current)}
          />
        </div>
      </div>

      <div className={`grid gap-2 ${installedProfile ? "grid-cols-2" : "grid-cols-1"}`}>
        <button
          type="button"
          className="inline-flex h-10 w-full cursor-pointer items-center justify-center gap-2 rounded-lg bg-zinc-900 px-4 text-sm font-medium text-white hover:bg-zinc-800 disabled:cursor-not-allowed disabled:opacity-50"
          onClick={handleRun}
          disabled={!canRun}
        >
          <Play size={16} fill="currentColor" />
          {running ? "Running" : "Run installer"}
        </button>
        {installedProfile ? (
          <button
            type="button"
            className="inline-flex h-10 w-full cursor-pointer items-center justify-center gap-2 rounded-lg border border-emerald-300 bg-emerald-50 px-3 text-sm font-medium text-emerald-800 hover:bg-emerald-100 disabled:cursor-not-allowed disabled:opacity-50"
            onClick={handleAddProfile}
            disabled={addingProfile}
          >
            <Server size={16} />
            {addingProfile ? "Adding" : "Add server"}
          </button>
        ) : null}
      </div>

      <div className="relative min-h-64 flex-1 overflow-hidden rounded-xl border border-zinc-800 bg-[#07090d] shadow-2xl shadow-zinc-950/15">
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
      <div className="h-1 shrink-0" />
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
        className={inputClass}
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
  visible,
  onChange,
  onToggle,
}: {
  label: string;
  value: string;
  visible: boolean;
  onChange: (value: string) => void;
  onToggle: () => void;
}) {
  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-zinc-500">{label}</p>
      <PasswordInput value={value} visible={visible} onChange={onChange} onToggle={onToggle} />
    </div>
  );
}

function PasswordInput({
  value,
  visible,
  onChange,
  onToggle,
}: {
  value: string;
  visible: boolean;
  onChange: (value: string) => void;
  onToggle: () => void;
}) {
  return (
    <div className="relative">
      <input
        className={passwordInputClass}
        type={visible ? "text" : "password"}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
      <button
        type="button"
        className="absolute right-2 top-1/2 inline-flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800"
        onClick={onToggle}
        aria-label={visible ? "Hide password" : "Show password"}
        aria-pressed={visible}
      >
        {visible ? <EyeOff size={16} /> : <Eye size={16} />}
      </button>
    </div>
  );
}

function FieldSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: string[];
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    function onClick(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  return (
    <div>
      <p className="mb-1.5 text-xs font-medium text-zinc-500">{label}</p>
      <div className="relative" ref={rootRef}>
        <button
          type="button"
          className={`${inputClass} flex cursor-pointer items-center justify-between gap-2 text-left`}
          onClick={() => setOpen((current) => !current)}
          aria-haspopup="listbox"
          aria-expanded={open}
        >
          <span className="truncate font-mono text-[13px]">{value}</span>
          <ChevronDown
            size={16}
            className={`shrink-0 text-zinc-400 transition ${open ? "rotate-180" : ""}`}
          />
        </button>
        {open ? (
          <ul
            role="listbox"
            className="absolute z-20 mt-1 max-h-56 w-full overflow-auto rounded-xl border border-zinc-200 bg-white p-1 shadow-lg"
          >
            {options.map((option) => {
              const selected = option === value;
              return (
                <li key={option}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={selected}
                    className={`flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-3 py-2 text-left font-mono text-[13px] ${selected ? "bg-zinc-900 text-white" : "text-zinc-800 hover:bg-zinc-100"
                      }`}
                    onClick={() => {
                      onChange(option);
                      setOpen(false);
                    }}
                  >
                    <span className="truncate">{option}</span>
                    {selected ? <Check size={14} className="shrink-0" /> : null}
                  </button>
                </li>
              );
            })}
          </ul>
        ) : null}
      </div>
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
