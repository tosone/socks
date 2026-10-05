import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ReactNode, SubmitEvent } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, Eye, EyeOff } from "lucide-react";
import type { Profile, ProfileInput } from "../types";

const MAX_PROFILE_NAME_CHARS = 10;

// The cipher list is portaled out of the dialog and kept to a few rows so the
// dialog itself never starts scrolling when the list opens.
const CIPHER_PANEL_ROWS = 4;
const CIPHER_PANEL_ROW_HEIGHT = 36; // h-9 on each option
const CIPHER_PANEL_PADDING = 8; // p-1 top + bottom
const CIPHER_PANEL_BORDER = 2; // 1px top + bottom
const CIPHER_PANEL_GAP = 4;
const CIPHER_PANEL_MAX_HEIGHT =
  CIPHER_PANEL_ROWS * CIPHER_PANEL_ROW_HEIGHT +
  CIPHER_PANEL_PADDING +
  CIPHER_PANEL_BORDER;

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
  const [method, setMethod] = useState(
    initial?.method ?? ciphers[0] ?? "2022-blake3-chacha20-poly1305",
  );
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
    });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <form className="max-h-[90vh] w-full max-w-sm overflow-y-auto rounded-2xl bg-white p-5 shadow-xl" onSubmit={handleSubmit}>
        <h2 className="text-lg font-semibold text-zinc-900">{title}</h2>
        <div className="mt-4 grid gap-4">
          <Field label="Name" error={errors.name}>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={MAX_PROFILE_NAME_CHARS}
              className="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-zinc-400"
            />
          </Field>
          <div className="grid grid-cols-3 gap-3">
            <Field label="Server" className="col-span-2" error={errors.server}>
              <input
                value={server}
                onChange={(e) => setServer(e.target.value)}
                className="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-zinc-400"
              />
            </Field>
            <Field label="Port" error={errors.port}>
              <input
                value={port}
                onChange={(e) => setPort(e.target.value)}
                className="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-zinc-400"
              />
            </Field>
          </div>
          <Field label="Password" error={errors.password}>
            <div className="relative">
              <input
                type={passwordVisible ? "text" : "password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 pr-10 text-sm text-zinc-900 outline-none focus:border-zinc-400"
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
            className="cursor-pointer rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white hover:bg-zinc-800 disabled:cursor-not-allowed disabled:opacity-60"
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
  const [placement, setPlacement] = useState<{
    left: number;
    width: number;
    top: number;
    placeAbove: boolean;
  } | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const panelRef = useRef<HTMLUListElement | null>(null);

  // Anchor the portaled list to the trigger and flip it above the trigger when
  // there is no room below, so it stays fully on screen.
  useLayoutEffect(() => {
    if (!open) {
      return;
    }
    function update() {
      const trigger = buttonRef.current;
      if (!trigger) {
        return;
      }
      const rect = trigger.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      const placeAbove =
        spaceBelow < CIPHER_PANEL_MAX_HEIGHT + CIPHER_PANEL_GAP + 8 &&
        rect.top > CIPHER_PANEL_MAX_HEIGHT + CIPHER_PANEL_GAP + 8;
      setPlacement({
        left: rect.left,
        width: rect.width,
        top: placeAbove ? rect.top : rect.bottom + CIPHER_PANEL_GAP,
        placeAbove,
      });
    }
    update();
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }
    function onPointerDown(event: MouseEvent) {
      const target = event.target as Node;
      if (rootRef.current?.contains(target) || panelRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="relative" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        className="flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg border border-zinc-200 bg-white px-3 py-2 text-left text-sm text-zinc-900 outline-none focus:border-zinc-400"
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
      {open && placement
        ? createPortal(
            <ul
              ref={panelRef}
              role="listbox"
              className="z-[60] overflow-y-auto rounded-xl border border-zinc-200 bg-white p-1 shadow-lg"
              style={{
                position: "fixed",
                left: placement.left,
                width: placement.width,
                top: placement.top,
                maxHeight: CIPHER_PANEL_MAX_HEIGHT,
                transform: placement.placeAbove
                  ? `translateY(calc(-100% - ${CIPHER_PANEL_GAP}px))`
                  : undefined,
              }}
            >
              {options.map((cipher) => {
                const selected = cipher === value;
                return (
                  <li key={cipher}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={selected}
                      className={`flex h-9 w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-3 text-left font-mono text-[13px] ${selected ? "bg-zinc-900 text-white" : "text-zinc-800 hover:bg-zinc-100"
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
            </ul>,
            document.body,
          )
        : null}
    </div>
  );
}
