import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
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
const INSTALLER_STATE_KEY = "socks.ssh.installerState";
const DEFAULT_PRIVATE_KEY_PATH = "~/.ssh/id_ed25519";
const DEFAULT_METHOD = "2022-blake3-chacha20-poly1305";

type LogEntry = {
  id: number;
  stream: SshRunEvent["stream"];
  data: string;
};

type InstallerState = {
  host: string;
  port: number;
  username: string;
  authMode: SshAuthMode;
  privateKeyPath: string;
  password: string;
  servicePassword: string;
  method: string;
  pluginDomain: string;
};

type SshRunnerProps = {
  ciphers: string[];
  onAddProfile: (input: ProfileInput) => Promise<void>;
};

export function SshRunner({ ciphers, onAddProfile }: SshRunnerProps) {
  const savedInstallerState = useMemo(() => loadInstallerState(), []);
  const [host, setHost] = useState(savedInstallerState.host ?? "");
  const [port, setPort] = useState(savedInstallerState.port ?? 22);
  const [username, setUsername] = useState(savedInstallerState.username ?? "root");
  const [authMode, setAuthMode] = useState<SshAuthMode>(savedInstallerState.authMode ?? "key");
  const [privateKeyPath, setPrivateKeyPath] = useState(
    savedInstallerState.privateKeyPath ?? DEFAULT_PRIVATE_KEY_PATH,
  );
  const [password, setPassword] = useState(savedInstallerState.password ?? "");
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [servicePassword, setServicePassword] = useState(
    savedInstallerState.servicePassword ?? "change-me",
  );
  const [servicePasswordVisible, setServicePasswordVisible] = useState(false);
  const [method, setMethod] = useState(
    savedInstallerState.method ?? ciphers[0] ?? DEFAULT_METHOD,
  );
  const [pluginDomain, setPluginDomain] = useState(savedInstallerState.pluginDomain ?? "");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [installedProfile, setInstalledProfile] = useState<ProfileInput | null>(null);
  const [addingProfile, setAddingProfile] = useState(false);
  const [follow, setFollow] = useState(true);
  const terminalRef = useRef<Terminal | null>(null);
  const runningRef = useRef(false);
  const nextIdRef = useRef(1);

  useEffect(() => {
    const savedPath = window.localStorage.getItem(PRIVATE_KEY_PATH_KEY);
    if (savedPath && !savedInstallerState.privateKeyPath) {
      setPrivateKeyPath(savedPath);
    }
  }, [savedInstallerState.privateKeyPath]);

  useEffect(() => {
    if (authMode === "key") {
      window.localStorage.setItem(PRIVATE_KEY_PATH_KEY, privateKeyPath);
    }
  }, [authMode, privateKeyPath]);

  useEffect(() => {
    window.localStorage.setItem(
      INSTALLER_STATE_KEY,
      JSON.stringify({
        host,
        port,
        username,
        authMode,
        privateKeyPath,
        password,
        servicePassword,
        method,
        pluginDomain,
      } satisfies InstallerState),
    );
  }, [
    authMode,
    host,
    method,
    password,
    pluginDomain,
    port,
    privateKeyPath,
    servicePassword,
    username,
  ]);

  useEffect(() => {
    if (ciphers.length === 0) {
      return;
    }
    if (!ciphers.includes(method)) {
      setMethod(ciphers[0] ?? DEFAULT_METHOD);
    }
  }, [ciphers, method]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<SshRunEvent>("ssh-run", (event) => {
      setLogs((current) => [
        ...current,
        {
          id: nextIdRef.current++,
          stream: event.payload.stream,
          data: event.payload.data,
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
    terminalRef.current?.scrollToBottom();
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
        pluginDomain: pluginDomain.trim() || null,
      });
      if (result.exitStatus === 0) {
        setInstalledProfile(buildProfileInput(result.pluginCertRaw ?? null));
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

  function buildProfileInput(pluginCertRaw: string | null): ProfileInput {
    const server = host.trim();
    const domain = pluginDomain.trim();
    const pluginOpts = domain
      ? pluginCertRaw
        ? `tls;host=${domain};certRaw=${pluginCertRaw}`
        : `tls;host=${domain}`
      : null;
    return {
      name: server.slice(0, 10) || "server",
      server,
      port: 443,
      password: servicePassword,
      method,
      plugin: domain ? "v2ray-plugin" : null,
      pluginOpts,
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

  function enableFollow() {
    setFollow(true);
    window.requestAnimationFrame(() => terminalRef.current?.scrollToBottom());
  }

  return (
    <section className="flex min-h-full flex-1 flex-col gap-3">
      <div className="rounded-xl border border-zinc-200 bg-white px-4 py-4 shadow-sm">
        <div>
          <div>
            <p className="mb-1.5 text-xs font-medium text-zinc-500">IP / Port</p>
            <div className="grid grid-cols-[1fr_5rem] gap-2">
              <input
                className="h-10 w-full rounded-lg border border-zinc-300 bg-white px-3 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
                value={host}
                placeholder="192.0.2.10"
                onChange={(event) => setHost(event.target.value)}
              />
              <input
                className="h-10 w-full appearance-none rounded-lg border border-zinc-300 bg-white px-3 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                min={1}
                max={65535}
                type="number"
                value={port}
                onChange={(event) => setPort(clampPort(event.target.valueAsNumber))}
              />
            </div>
          </div>
        </div>

        <div className="mt-3 grid grid-cols-[7.5rem_1fr_6.5rem] items-end gap-2">
          <FieldText
            label="User"
            value={username}
            placeholder="root"
            onChange={setUsername}
          />
          <div>
            <p className="mb-1.5 text-xs font-medium text-zinc-500">
              {authMode === "key" ? "Private key path" : "SSH password"}
            </p>
            {authMode === "key" ? (
              <input
                className="h-10 w-full rounded-lg border border-zinc-300 bg-white px-3 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
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
          </div>
          <button
            type="button"
            className="inline-flex h-10 w-full cursor-pointer items-center justify-center gap-2 rounded-lg border border-zinc-300 bg-white px-3 text-sm font-medium text-zinc-700 hover:bg-zinc-100"
            onClick={() => setAuthMode((current) => (current === "key" ? "password" : "key"))}
          >
            {authMode === "key" ? "Password" : "Key"}
          </button>
        </div>

        <div className="mt-4 border-t border-zinc-200" />

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

        <div className="mt-3">
          <FieldText
            label="Plugin domain"
            value={pluginDomain}
            placeholder="example.com"
            onChange={setPluginDomain}
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
        <div className="h-full pb-0 pl-3 pr-1 pt-3">
          <div className="h-full font-mono text-[12px] leading-5 text-zinc-100">
            <TerminalOutput
              logs={logs}
              follow={follow}
              terminalRef={terminalRef}
              onFollowChange={setFollow}
            />
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
        className="h-10 w-full rounded-lg border border-zinc-300 bg-white py-0 pl-3 pr-10 text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
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
          className="flex h-10 w-full cursor-pointer items-center justify-between gap-2 rounded-lg border border-zinc-300 bg-white px-3 text-left text-sm text-zinc-900 outline-none transition focus:border-zinc-500 focus:ring-2 focus:ring-zinc-200"
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

function loadInstallerState(): Partial<InstallerState> {
  const raw = window.localStorage.getItem(INSTALLER_STATE_KEY);
  if (!raw) {
    return {};
  }
  try {
    const parsed = JSON.parse(raw) as Partial<InstallerState>;
    return {
      host: typeof parsed.host === "string" ? parsed.host : undefined,
      port: typeof parsed.port === "number" ? clampPort(parsed.port) : undefined,
      username: typeof parsed.username === "string" ? parsed.username : undefined,
      authMode:
        parsed.authMode === "key" || parsed.authMode === "password" ? parsed.authMode : undefined,
      privateKeyPath:
        typeof parsed.privateKeyPath === "string" ? parsed.privateKeyPath : undefined,
      password: typeof parsed.password === "string" ? parsed.password : undefined,
      servicePassword:
        typeof parsed.servicePassword === "string" ? parsed.servicePassword : undefined,
      method: typeof parsed.method === "string" ? parsed.method : undefined,
      pluginDomain: typeof parsed.pluginDomain === "string" ? parsed.pluginDomain : undefined,
    };
  } catch {
    return {};
  }
}

function TerminalOutput({
  logs,
  follow,
  terminalRef,
  onFollowChange,
}: {
  logs: LogEntry[];
  follow: boolean;
  terminalRef: RefObject<Terminal | null>;
  onFollowChange: (follow: boolean) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const lastLogIdRef = useRef(0);
  const pendingNewlineRef = useRef(false);
  const followRef = useRef(follow);

  useEffect(() => {
    followRef.current = follow;
    if (follow) {
      terminalRef.current?.scrollToBottom();
    }
  }, [follow, terminalRef]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const terminal = new Terminal({
      allowTransparency: true,
      convertEol: true,
      cursorBlink: false,
      cursorInactiveStyle: "none",
      disableStdin: true,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      fontSize: 12,
      lineHeight: 1.45,
      scrollback: 5000,
      theme: {
        background: "#07090d",
        foreground: "#f4f4f5",
        cursor: "#f4f4f5",
        black: "#3f3f46",
        red: "#f87171",
        green: "#4ade80",
        yellow: "#facc15",
        blue: "#60a5fa",
        magenta: "#f472b6",
        cyan: "#22d3ee",
        white: "#e4e4e7",
        brightBlack: "#71717a",
        brightRed: "#fca5a5",
        brightGreen: "#86efac",
        brightYellow: "#fde047",
        brightBlue: "#93c5fd",
        brightMagenta: "#f9a8d4",
        brightCyan: "#67e8f9",
        brightWhite: "#fafafa",
      },
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(container);
    fitAddon.fit();
    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    const scrollSubscription = terminal.onScroll(() => {
      if (terminal.buffer.active.viewportY < terminal.buffer.active.baseY) {
        onFollowChange(false);
      }
    });
    const resizeObserver = new ResizeObserver(() => fitAddon.fit());
    resizeObserver.observe(container);

    return () => {
      scrollSubscription.dispose();
      resizeObserver.disconnect();
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
      lastLogIdRef.current = 0;
      pendingNewlineRef.current = false;
    };
  }, [onFollowChange, terminalRef]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) {
      return;
    }

    if (logs.length === 0) {
      terminal.reset();
      lastLogIdRef.current = 0;
      pendingNewlineRef.current = false;
      return;
    }

    const lastLogId = lastLogIdRef.current;
    const firstNewLogIndex = logs.findIndex((log) => log.id > lastLogId);
    if (firstNewLogIndex === -1) {
      return;
    }

    const newLogs = logs.slice(firstNewLogIndex);
    for (const [index, log] of newLogs.entries()) {
      const trimTrailingNewline = index === newLogs.length - 1;
      terminal.write(formatTerminalLog(log, trimTrailingNewline, pendingNewlineRef.current), () => {
        if (followRef.current) {
          terminal.scrollToBottom();
        }
      });
      pendingNewlineRef.current = trimTrailingNewline && /[\r\n]+$/.test(log.data);
      lastLogIdRef.current = log.id;
    }
  }, [logs, terminalRef]);

  return <div ref={containerRef} className="h-full min-h-full [&_.xterm]:h-full [&_.xterm-viewport]:!overflow-y-auto" />;
}

function formatTerminalLog(log: LogEntry, trimTrailingNewline = false, prependNewline = false) {
  const text = trimTrailingNewline ? log.data.replace(/[\r\n]+$/, "") : log.data;
  const data = prependNewline ? `\n${text}` : text;
  if (log.stream !== "stderr") {
    return data;
  }
  return `\x1b[91m${data}\x1b[0m`;
}
