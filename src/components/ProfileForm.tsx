import { useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode, SubmitEvent } from "react";
import { Check, ChevronDown, Eye, EyeOff } from "lucide-react";
import type { Profile, ProfileInput } from "../types";

const MAX_PROFILE_NAME_CHARS = 10;

type ProfileFormProps = {
  title: string;
  ciphers: string[];
  initial?: Profile | null;
  busy?: boolean;
  error?: string | null;
  onSubmit: (input: ProfileInput) => Promise<void> | void;
  onCancel: () => void;
};

type FormErrors = Partial<Record<keyof ProfileInput, string>>;

export function ProfileForm({
  title,
  ciphers,
  initial,
  busy = false,
  error,
  onSubmit,
  onCancel,
}: ProfileFormProps) {
  const [name, setName] = useState(initial?.name ?? "");
  const [server, setServer] = useState(initial?.server ?? "");
  const [port, setPort] = useState(String(initial?.port ?? 8388));
  const [password, setPassword] = useState(initial?.password ?? "");
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [method, setMethod] = useState(initial?.method ?? ciphers[0] ?? "aes-256-gcm");
  const [plugin, setPlugin] = useState(initial?.plugin ?? "");
  const [pluginOpts, setPluginOpts] = useState(initial?.pluginOpts ?? "");
  const [errors, setErrors] = useState<FormErrors>({});

  const cipherOptions = useMemo(
    () => (ciphers.includes(method) ? ciphers : [method, ...ciphers]),
    [ciphers, method],
  );

  async function handleSubmit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextErrors: FormErrors = {};
    const parsedPort = Number(port);
    if (!name.trim()) {
      nextErrors.name = "Name is required";
    } else if (Array.from(name.trim()).length > MAX_PROFILE_NAME_CHARS) {
      nextErrors.name = `Name must be ${MAX_PROFILE_NAME_CHARS} characters or fewer`;
    }
    if (!server.trim()) {
      nextErrors.server = "Server is required";
    }
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      nextErrors.port = "Port must be 1–65535";
    }
    if (!password) {
      nextErrors.password = "Password is required";
    }
    if (!method) {
      nextErrors.method = "Select an encryption method";
    }
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) {
      return;
    }
    await onSubmit({
      name: name.trim(),
      server: server.trim(),
      port: parsedPort,
      password,
      method,
      plugin: plugin.trim() || null,
      pluginOpts: pluginOpts.trim() || null,
    });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <form
        className="max-h-[90vh] w-full max-w-sm overflow-y-auto rounded-2xl bg-white p-5 shadow-xl"
        onSubmit={handleSubmit}
      >
        <h2 className="text-lg font-semibold text-zinc-900">{title}</h2>
        <div className="mt-4 grid gap-4">
          <Field label="Name" error={errors.name}>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={MAX_PROFILE_NAME_CHARS}
              className={inputClass}
            />
          </Field>
          <div className="grid grid-cols-3 gap-3">
            <Field label="Server" className="col-span-2" error={errors.server}>
              <input value={server} onChange={(e) => setServer(e.target.value)} className={inputClass} />
            </Field>
            <Field label="Port" error={errors.port}>
              <input value={port} onChange={(e) => setPort(e.target.value)} className={inputClass} />
            </Field>
          </div>
          <Field label="Password" error={errors.password}>
            <div className="relative">
              <input
                type={passwordVisible ? "text" : "password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className={`${inputClass} pr-10`}
              />
              <button
                type="button"
                className="absolute right-2 top-1/2 inline-flex h-7 w-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-700 focus:outline-none focus:ring-2 focus:ring-zinc-300"
                onClick={() => setPasswordVisible((current) => !current)}
                aria-label={passwordVisible ? "Hide password" : "Show password"}
                aria-pressed={passwordVisible}
              >
                {passwordVisible ? <EyeOff size={16} /> : <Eye size={16} />}
              </button>
            </div>
          </Field>
          <Field label="Encryption" error={errors.method}>
            <CipherSelect value={method} options={cipherOptions} onChange={setMethod} />
          </Field>
          <Field label="Plugin (optional)">
            <input
              value={plugin}
              onChange={(e) => setPlugin(e.target.value)}
              placeholder="v2ray-plugin"
              className={inputClass}
            />
          </Field>
          <Field label="Plugin options (optional)">
            <input
              value={pluginOpts}
              onChange={(e) => setPluginOpts(e.target.value)}
              placeholder="obfs=http;obfs-host=www.example.com"
              className={inputClass}
            />
          </Field>
        </div>
        {error ? <p className="mt-3 text-sm text-red-600">{error}</p> : null}
        <div className="mt-6 flex justify-end gap-2">
          <button
            type="button"
            className="cursor-pointer rounded-lg border border-zinc-200 px-4 py-2 text-sm text-zinc-700 hover:bg-zinc-50"
            onClick={onCancel}
            disabled={busy}
          >
            Cancel
          </button>
          <button
            type="submit"
            className="cursor-pointer rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white hover:bg-zinc-800 disabled:opacity-60 disabled:cursor-not-allowed"
            disabled={busy}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </div>
  );
}

function Field({
  label,
  error,
  className = "",
  children,
}: {
  label: string;
  error?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`block text-sm ${className}`}>
      <span className="mb-1 block font-medium text-zinc-700">{label}</span>
      {children}
      {error ? <span className="mt-1 block text-xs text-red-600">{error}</span> : null}
    </div>
  );
}

const inputClass =
  "w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-zinc-400";
function CipherSelect({
  value,
  options,
  onChange,
}: {
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
          {options.map((cipher) => {
            const selected = cipher === value;
            return (
              <li key={cipher}>
                <button
                  type="button"
                  role="option"
                  aria-selected={selected}
                  className={`flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-3 py-2 text-left font-mono text-[13px] ${selected ? "bg-zinc-900 text-white" : "text-zinc-800 hover:bg-zinc-100"
                    }`}
                  onClick={() => {
                    onChange(cipher);
                    setOpen(false);
                  }}
                >
                  <span className="truncate">{cipher}</span>
                  {selected ? <Check size={14} className="shrink-0" /> : null}
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
