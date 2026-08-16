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
  tunName: string | null;
  helperInstalled: boolean;
};

export type HelperStatus = {
  installed: boolean;
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
  samples: SpeedSample[];
};

export type SpeedSample = {
  up: number;
  down: number;
};
