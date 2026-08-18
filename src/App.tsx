import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Plus } from "lucide-react";
import {
  connect,
  createProfile,
  deleteProfile,
  disconnect,
  installHelper,
  listCiphers,
  listProfiles,
  listTrafficTotals,
  runtimeStatus,
  uninstallHelper,
  updateProfile,
} from "./api";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ErrorDialog } from "./components/ErrorDialog";
import { ProfileCard } from "./components/ProfileCard";
import { ProfileForm } from "./components/ProfileForm";
import { SshRunner } from "./components/SshRunner";
import type {
  ConnectivityEvent,
  ConnectivityStatus,
  Profile,
  ProfileInput,
  SpeedSample,
  TrafficEvent,
} from "./types";

const TRAFFIC_SAMPLE_INTERVAL_MS = 10_000;
const MOCK_SAMPLE_LIMIT = (30 * 60 * 1000) / TRAFFIC_SAMPLE_INTERVAL_MS;
const SHOW_MOCK_PROFILES = true;
const MOCK_PROFILE_ID_PREFIX = "mock-";
const MOCK_PROFILES: Profile[] = [
  {
    id: "mock-tokyo",
    name: "Tokyo edge",
    server: "203.0.113.24",
    port: 8388,
    password: "",
    method: "2022-blake3-aes-256-gcm",
    plugin: null,
    pluginOpts: null,
    createdAt: 0,
  },
  {
    id: "mock-singapore",
    name: "Singapore relay",
    server: "198.51.100.18",
    port: 443,
    password: "",
    method: "chacha20-ietf-poly1305",
    plugin: null,
    pluginOpts: null,
    createdAt: 1,
  },
  {
    id: "mock-frankfurt",
    name: "Frankfurt office",
    server: "192.0.2.84",
    port: 8443,
    password: "",
    method: "aes-256-gcm",
    plugin: null,
    pluginOpts: null,
    createdAt: 2,
  },
  {
    id: "mock-seattle",
    name: "Seattle backup",
    server: "203.0.113.91",
    port: 9001,
    password: "",
    method: "2022-blake3-chacha20-poly1305",
    plugin: null,
    pluginOpts: null,
    createdAt: 3,
  },
  {
    id: "mock-london",
    name: "London home",
    server: "198.51.100.42",
    port: 8388,
    password: "",
    method: "aes-128-gcm",
    plugin: null,
    pluginOpts: null,
    createdAt: 4,
  },
  {
    id: "mock-sydney",
    name: "Sydney lab",
    server: "192.0.2.129",
    port: 1443,
    password: "",
    method: "xchacha20-ietf-poly1305",
    plugin: null,
    pluginOpts: null,
    createdAt: 5,
  },
];
const INITIAL_SAMPLE_SPEED = { up: 42_300, down: 218_900 };
const INITIAL_SAMPLE_SPEED_SAMPLES: SpeedSample[] = [
  { up: 8_200, down: 44_000 },
  { up: 16_800, down: 92_500 },
  { up: 11_400, down: 74_200 },
  { up: 28_100, down: 168_300 },
  { up: 22_500, down: 137_600 },
  { up: 35_900, down: 201_200 },
  { up: 31_200, down: 186_700 },
  { up: 48_600, down: 252_400 },
  { up: 39_800, down: 224_900 },
  { up: INITIAL_SAMPLE_SPEED.up, down: INITIAL_SAMPLE_SPEED.down },
];

type Page = "profiles" | "ssh";

function nextSampleSpeed(current: { up: number; down: number }) {
  const up = Math.round(clamp(current.up * randomBetween(0.65, 1.45), 12_000, 76_000));
  const down = Math.round(clamp(current.down * randomBetween(0.7, 1.35), 80_000, 360_000));
  return { up, down };
}

function randomBetween(min: number, max: number) {
  return min + Math.random() * (max - min);
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function initialMockSpeeds() {
  return Object.fromEntries(
    MOCK_PROFILES.map((profile, index) => [
      profile.id,
      {
        up: Math.round(INITIAL_SAMPLE_SPEED.up * (0.72 + index * 0.08)),
        down: Math.round(INITIAL_SAMPLE_SPEED.down * (0.7 + index * 0.07)),
      },
    ]),
  );
}

function initialMockSamples() {
  return Object.fromEntries(MOCK_PROFILES.map((profile) => [profile.id, INITIAL_SAMPLE_SPEED_SAMPLES]));
}

function initialMockTotals() {
  return Object.fromEntries(MOCK_PROFILES.map((profile) => [profile.id, { up: 0, down: 0 }]));
}

function isMockProfile(profile: Profile) {
  return profile.id.startsWith(MOCK_PROFILE_ID_PREFIX);
}

function mockConnectivityResult(profileId: string): ConnectivityStatus {
  return profileId === "mock-seattle" ? "failed" : "connected";
}

export default function App() {
  const [page, setPage] = useState<Page>("profiles");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [mockProfiles, setMockProfiles] = useState<Profile[]>(MOCK_PROFILES);
  const [mockActiveId, setMockActiveId] = useState<string | null>(MOCK_PROFILES[0]?.id ?? null);
  const [mockSpeeds, setMockSpeeds] = useState<Record<string, { up: number; down: number }>>(initialMockSpeeds);
  const [mockSamples, setMockSamples] = useState<Record<string, SpeedSample[]>>(initialMockSamples);
  const [mockTotals, setMockTotals] = useState<Record<string, { up: number; down: number }>>(initialMockTotals);
  const [mockConnectivity, setMockConnectivity] = useState<Record<string, ConnectivityStatus>>(
    MOCK_PROFILES[0] ? { [MOCK_PROFILES[0].id]: "connected" } : {},
  );
  const [ciphers, setCiphers] = useState<string[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Profile | null>(null);
  const [formError, setFormError] = useState<string | null>(null);
  const [formBusy, setFormBusy] = useState(false);
  const [deleting, setDeleting] = useState<Profile | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [errorDialog, setErrorDialog] = useState<string | null>(null);
  const [helperInstalled, setHelperInstalled] = useState(false);
  const [helperBusy, setHelperBusy] = useState(false);
  const [speeds, setSpeeds] = useState<Record<string, { up: number; down: number }>>({});
  const [samples, setSamples] = useState<Record<string, SpeedSample[]>>({});
  const [totals, setTotals] = useState<Record<string, { up: number; down: number }>>({});
  const [connectivity, setConnectivity] = useState<Record<string, ConnectivityStatus>>({});

  async function refresh() {
    const [nextProfiles, status, nextCiphers, nextTotals] = await Promise.all([
      listProfiles(),
      runtimeStatus(),
      listCiphers(),
      listTrafficTotals(),
    ]);
    setHelperInstalled(status.helperInstalled);
    setProfiles(nextProfiles);
    setActiveId(status.activeProfileId);
    setCiphers(nextCiphers);
    setTotals(
      Object.fromEntries(
        nextProfiles.map((profile) => [
          profile.id,
          { up: nextTotals[profile.id]?.tx ?? 0, down: nextTotals[profile.id]?.rx ?? 0 },
        ]),
      ),
    );
  }

  useEffect(() => {
    refresh().catch((err) => setErrorDialog(String(err)));
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<ConnectivityEvent>("connectivity", (event) => {
      const payload = event.payload;
      setConnectivity((current) => ({
        ...current,
        [payload.profileId]: payload.status,
      }));
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => setErrorDialog(String(err)));
    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (mockProfiles.length === 0) {
      return;
    }

    const timer = window.setInterval(() => {
      setMockSpeeds((current) => {
        const next = { ...current };
        for (const profile of mockProfiles) {
          if (mockActiveId !== profile.id) {
            next[profile.id] = { up: 0, down: 0 };
            continue;
          }
          next[profile.id] = nextSampleSpeed(current[profile.id] ?? INITIAL_SAMPLE_SPEED);
        }
        setMockSamples((currentSamples) => {
          const nextSamples = { ...currentSamples };
          for (const profile of mockProfiles) {
            const speed = next[profile.id] ?? { up: 0, down: 0 };
            nextSamples[profile.id] = [...(currentSamples[profile.id] ?? []), speed].slice(-MOCK_SAMPLE_LIMIT);
          }
          return nextSamples;
        });
        setMockTotals((currentTotals) => {
          const nextTotals = { ...currentTotals };
          for (const profile of mockProfiles) {
            if (mockActiveId !== profile.id) {
              continue;
            }
            const speed = next[profile.id] ?? { up: 0, down: 0 };
            nextTotals[profile.id] = {
              up: (currentTotals[profile.id]?.up ?? 0) + speed.up * (TRAFFIC_SAMPLE_INTERVAL_MS / 1000),
              down: (currentTotals[profile.id]?.down ?? 0) + speed.down * (TRAFFIC_SAMPLE_INTERVAL_MS / 1000),
            };
          }
          return nextTotals;
        });
        return next;
      });
    }, TRAFFIC_SAMPLE_INTERVAL_MS);

    return () => window.clearInterval(timer);
  }, [mockActiveId, mockProfiles]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<TrafficEvent>("traffic", (event) => {
      const payload = event.payload;
      setSpeeds((current) => ({
        ...current,
        [payload.profileId]: { up: payload.upBps, down: payload.downBps },
      }));
      setTotals((current) => ({
        ...current,
        [payload.profileId]: { up: payload.totalTx, down: payload.totalRx },
      }));
      setSamples((current) => {
        return { ...current, [payload.profileId]: payload.samples };
      });
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => setErrorDialog(String(err)));
    return () => {
      unlisten?.();
    };
  }, []);

  const formTitle = editing ? "Edit profile" : "New profile";

  const sortedProfiles = useMemo(
    () => [...profiles].sort((a, b) => a.createdAt - b.createdAt),
    [profiles],
  );

  function openCreateForm() {
    setEditing(null);
    setFormError(null);
    setFormOpen(true);
  }

  async function handleSave(input: ProfileInput) {
    setFormBusy(true);
    setFormError(null);
    try {
      if (editing && isMockProfile(editing)) {
        setMockProfiles((current) =>
          current.map((profile) =>
            profile.id === editing.id
              ? {
                ...profile,
                name: input.name.trim(),
                server: input.server.trim(),
                port: input.port,
                password: input.password,
                method: input.method,
                plugin: input.plugin?.trim() || null,
                pluginOpts: input.pluginOpts?.trim() || null,
              }
              : profile,
          ),
        );
      } else if (editing) {
        await updateProfile(editing.id, input);
      } else {
        await createProfile(input);
      }
      setFormOpen(false);
      setEditing(null);
      await refresh();
    } catch (err) {
      setFormError(String(err));
    } finally {
      setFormBusy(false);
    }
  }

  async function handleAddInstalledProfile(input: ProfileInput) {
    await createProfile(input);
    await refresh();
    setPage("profiles");
  }

  async function handleToggle(profile: Profile) {
    setBusyId(profile.id);
    setErrorDialog(null);
    try {
      if (activeId === profile.id) {
        const status = await disconnect();
        setActiveId(status.activeProfileId);
        setHelperInstalled(status.helperInstalled);
        setSpeeds((current) => ({ ...current, [profile.id]: { up: 0, down: 0 } }));
        setSamples((current) => ({ ...current, [profile.id]: [] }));
        setConnectivity((current) => {
          const next = { ...current };
          delete next[profile.id];
          return next;
        });
      } else {
        setConnectivity({ [profile.id]: "checking" });
        const status = await connect(profile.id);
        setActiveId(status.activeProfileId);
        setHelperInstalled(status.helperInstalled);
        setSpeeds({ [profile.id]: { up: 0, down: 0 } });
        setSamples({ [profile.id]: [] });
      }
    } catch (err) {
      setConnectivity({ [profile.id]: "failed" });
      setErrorDialog(String(err));
      const status = await runtimeStatus().catch(() => null);
      if (status) {
        setActiveId(status.activeProfileId);
        setHelperInstalled(status.helperInstalled);
      }
    } finally {
      setBusyId(null);
    }
  }

  async function handleDelete() {
    if (!deleting) {
      return;
    }
    setDeleteBusy(true);
    setErrorDialog(null);
    try {
      if (isMockProfile(deleting)) {
        setMockProfiles((current) => current.filter((profile) => profile.id !== deleting.id));
        setMockActiveId((current) => (current === deleting.id ? null : current));
        setMockConnectivity((current) => {
          const next = { ...current };
          delete next[deleting.id];
          return next;
        });
        setMockTotals((current) => {
          const next = { ...current };
          delete next[deleting.id];
          return next;
        });
      } else {
        await deleteProfile(deleting.id);
        setTotals((current) => {
          const next = { ...current };
          delete next[deleting.id];
          return next;
        });
        await refresh();
      }
      setDeleting(null);
    } catch (err) {
      setErrorDialog(String(err));
    } finally {
      setDeleteBusy(false);
    }
  }

  async function handleInstallHelper() {
    setHelperBusy(true);
    setErrorDialog(null);
    try {
      const status = await installHelper();
      setHelperInstalled(status.installed);
    } catch (err) {
      setErrorDialog(String(err));
    } finally {
      setHelperBusy(false);
    }
  }

  async function handleUninstallHelper() {
    setHelperBusy(true);
    setErrorDialog(null);
    try {
      const status = await uninstallHelper();
      setHelperInstalled(status.installed);
    } catch (err) {
      setErrorDialog(String(err));
    } finally {
      setHelperBusy(false);
    }
  }

  function togglePage() {
    setPage((current) => (current === "profiles" ? "ssh" : "profiles"));
  }

  useEffect(() => {
    window.scrollTo({ top: 0 });
  }, [page]);

  return (
    <div className="min-h-screen bg-zinc-50 text-zinc-900">
      <header className="fixed inset-x-0 top-0 z-20 border-b border-zinc-200 bg-white/90 backdrop-blur">
        <div
          className="mx-auto flex max-w-3xl items-center justify-between px-4 py-3"
          onClick={togglePage}
        >
          <div className="min-w-0">
            <p className="select-none text-sm font-medium text-zinc-600">
              {page === "profiles" ? "Shadowsocks client" : "Installer"}
            </p>
          </div>
          {page === "profiles" ? (
            <button
              type="button"
              className="inline-flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg bg-zinc-900 text-white hover:bg-zinc-800"
              onClick={(event) => {
                event.stopPropagation();
                openCreateForm();
              }}
              aria-label="Add profile"
            >
              <Plus size={18} />
            </button>
          ) : (
            <div className="h-9 w-9" aria-hidden="true" />
          )}
        </div>
      </header>

      <div
        className={`relative [perspective:1600px] ${page === "ssh" ? "h-screen overflow-hidden" : "min-h-screen"}`}
      >
        <main
          className={`${page === "profiles" ? "relative" : "absolute h-screen overflow-hidden"} inset-x-0 top-0 mx-auto flex max-w-3xl flex-col gap-4 px-4 pb-4 pt-[77px] transition duration-700 [backface-visibility:hidden] [transform-style:preserve-3d] ${page === "profiles"
            ? "pointer-events-auto opacity-100 [transform:rotateY(0deg)]"
            : "pointer-events-none opacity-0 [transform:rotateY(180deg)]"
            }`}
        >
          <div
            className={`rounded-xl border px-4 py-3 text-sm ${helperInstalled
              ? "border-emerald-200 bg-emerald-50 text-emerald-800"
              : "border-amber-200 bg-amber-50 text-amber-900"
              }`}
          >
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <p className="font-medium">
                  {helperInstalled ? "Helper installed" : "Helper required"}
                </p>
                <p className="mt-1 text-xs opacity-80">
                  {helperInstalled
                    ? "Routes are changed by a background helper. Connecting no longer asks for a password."
                    : "Install once; future connects skip admin prompts."}
                </p>
              </div>
              {helperInstalled ? (
                <button
                  type="button"
                  className="rounded-lg border border-emerald-300 bg-white px-3 py-1.5 text-xs text-emerald-800 hover:bg-emerald-100 disabled:opacity-60"
                  onClick={handleUninstallHelper}
                  disabled={helperBusy}
                >
                  {helperBusy ? "Working…" : "Uninstall"}
                </button>
              ) : (
                <button
                  type="button"
                  className="rounded-lg bg-zinc-900 px-3 py-1.5 text-xs text-white hover:bg-zinc-800 disabled:opacity-60"
                  onClick={handleInstallHelper}
                  disabled={helperBusy}
                >
                  {helperBusy ? "Installing…" : "Install helper"}
                </button>
              )}
            </div>
          </div>

          {errorDialog ? (
            <ErrorDialog
              message={errorDialog}
              onClose={() => setErrorDialog(null)}
            />
          ) : null}

          {sortedProfiles.length === 0 ? (
            <>
              <button
                type="button"
                className="cursor-pointer rounded-2xl border border-dashed border-zinc-300 bg-white px-6 py-16 text-center text-sm text-zinc-500 transition hover:border-zinc-400 hover:bg-zinc-100 hover:text-zinc-700 focus:outline-none focus:ring-2 focus:ring-zinc-300 focus:ring-offset-2"
                onClick={openCreateForm}
              >
                No profiles yet. Click + to add a Shadowsocks server.
              </button>
              {SHOW_MOCK_PROFILES
                ? mockProfiles.map((profile, index) => {
                  const connected = mockActiveId === profile.id;
                  const speed = mockSpeeds[profile.id] ?? { up: 0, down: 0 };
                  const total = mockTotals[profile.id] ?? { up: 0, down: 0 };
                  const mockStatus = mockConnectivity[profile.id];
                  return (
                    <ProfileCard
                      key={profile.id}
                      profile={profile}
                      connected={connected}
                      connecting={mockStatus === "checking"}
                      upBps={connected ? speed.up : 0}
                      downBps={connected ? speed.down : 0}
                      totalUpBytes={total.up}
                      totalDownBytes={total.down}
                      samples={mockSamples[profile.id] ?? []}
                      connectivityStatus={connected ? mockStatus ?? "checking" : undefined}
                      menuPlacement={index === mockProfiles.length - 1 ? "up" : "down"}
                      onToggle={() => {
                        setMockActiveId((current) => {
                          if (current === profile.id) {
                            setMockSpeeds((speeds) => ({ ...speeds, [profile.id]: { up: 0, down: 0 } }));
                            setMockSamples((samples) => ({ ...samples, [profile.id]: [] }));
                            setMockConnectivity((statuses) => {
                              const next = { ...statuses };
                              delete next[profile.id];
                              return next;
                            });
                            return null;
                          }
                          setMockSpeeds((speeds) => ({
                            ...speeds,
                            ...(current ? { [current]: { up: 0, down: 0 } } : {}),
                          }));
                          setMockSamples((samples) => ({
                            ...samples,
                            ...(current ? { [current]: [] } : {}),
                            [profile.id]: [],
                          }));
                          setMockConnectivity({
                            [profile.id]: "checking",
                          });
                          window.setTimeout(() => {
                            setMockConnectivity((statuses) => {
                              if (statuses[profile.id] !== "checking") {
                                return statuses;
                              }
                              return {
                                ...statuses,
                                [profile.id]: mockConnectivityResult(profile.id),
                              };
                            });
                          }, 1200);
                          return profile.id;
                        });
                      }}
                      onEdit={() => {
                        setEditing(profile);
                        setFormError(null);
                        setFormOpen(true);
                      }}
                      onDelete={() => setDeleting(profile)}
                    />
                  );
                })
                : null}
            </>
          ) : (
            sortedProfiles.map((profile, index) => {
              const connected = activeId === profile.id;
              const speed = speeds[profile.id] ?? { up: 0, down: 0 };
              const total = totals[profile.id] ?? { up: 0, down: 0 };
              return (
                <ProfileCard
                  key={profile.id}
                  profile={profile}
                  connected={connected}
                  connecting={busyId === profile.id}
                  upBps={connected ? speed.up : 0}
                  downBps={connected ? speed.down : 0}
                  totalUpBytes={total.up}
                  totalDownBytes={total.down}
                  samples={samples[profile.id] ?? []}
                  connectivityStatus={
                    connected || busyId === profile.id
                      ? connectivity[profile.id] ?? "checking"
                      : undefined
                  }
                  menuPlacement={index === sortedProfiles.length - 1 ? "up" : "down"}
                  onToggle={() => handleToggle(profile)}
                  onEdit={() => {
                    setEditing(profile);
                    setFormError(null);
                    setFormOpen(true);
                  }}
                  onDelete={() => setDeleting(profile)}
                />
              );
            })
          )}
        </main>

        <main
          className={`${page === "ssh" ? "relative" : "absolute"} inset-x-0 top-0 mx-auto flex h-screen max-w-3xl flex-col overflow-y-auto px-4 pb-6 pt-[77px] transition duration-700 [backface-visibility:hidden] [transform-style:preserve-3d] ${page === "ssh"
            ? "pointer-events-auto opacity-100 [transform:rotateY(0deg)]"
            : "pointer-events-none opacity-0 [transform:rotateY(-180deg)]"
            }`}
        >
          <SshRunner ciphers={ciphers} onAddProfile={handleAddInstalledProfile} />
        </main>
      </div>

      {formOpen ? (
        <ProfileForm
          title={formTitle}
          ciphers={ciphers}
          initial={editing}
          busy={formBusy}
          error={formError}
          onSubmit={handleSave}
          onCancel={() => {
            setFormOpen(false);
            setEditing(null);
          }}
        />
      ) : null}

      {deleting ? (
        <ConfirmDialog
          title="Delete profile"
          message="This cannot be undone. If the profile is connected, it will be disconnected first."
          busy={deleteBusy}
          onCancel={() => setDeleting(null)}
          onConfirm={handleDelete}
        />
      ) : null}
    </div>
  );
}
