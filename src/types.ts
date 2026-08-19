export type Profile = {
  id: string;
  name: string;
  server: string;
  port: number;
  password: string;
  method: string;
  plugin?: string | null;
  pluginOpts?: string | null;
  createdAt: number;
};

export type ProfileInput = {
  name: string;
  server: string;
  port: number;
  password: string;
  method: string;
  plugin?: string | null;
  pluginOpts?: string | null;
};

export type RuntimeStatus = {
  activeProfileId: string | null;
};

export type TrafficTotals = {
  tx: number;
  rx: number;
};

export type TrafficEvent = {
  profileId: string;
  tx: number;
  rx: number;
  upBps: number;
  downBps: number;
  totalTx: number;
  totalRx: number;
};

export type ConnectivityStatus = "checking" | "connected" | "failed";

export type ConnectivityEvent = {
  profileId: string;
  status: ConnectivityStatus;
  message?: string | null;
};

export type SshAuthMode = "key" | "password";

export type SshRunInput = {
  host: string;
  port: number;
  username: string;
  authMode: SshAuthMode;
  privateKeyPath?: string | null;
  password?: string | null;
  servicePassword: string;
  method: string;
  pluginDomain?: string | null;
};

export type SshRunEvent = {
  stream: "stdout" | "stderr" | "system";
  data: string;
};

export type SshRunResult = {
  exitStatus: number | null;
  pluginCertRaw?: string | null;
};
