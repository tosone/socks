import { invoke } from "@tauri-apps/api/core";
import type {
  Profile,
  ProfileInput,
  RuntimeStatus,
  SshRunInput,
  SshRunResult,
  TrafficTotals,
} from "./types";

export function listProfiles() {
  return invoke<Profile[]>("list_profiles");
}

export function listTrafficTotals() {
  return invoke<Record<string, TrafficTotals>>("list_traffic_totals");
}

export function createProfile(input: ProfileInput) {
  return invoke<Profile>("create_profile", { input });
}

export function updateProfile(id: string, input: ProfileInput) {
  return invoke<Profile>("update_profile", { id, input });
}

export function deleteProfile(id: string) {
  return invoke<void>("delete_profile", { id });
}

export function listCiphers() {
  return invoke<string[]>("list_ciphers");
}

export function connect(id: string) {
  return invoke<RuntimeStatus>("connect", { id });
}

export function disconnect() {
  return invoke<RuntimeStatus>("disconnect");
}

export function runtimeStatus() {
  return invoke<RuntimeStatus>("runtime_status");
}

export function runSshSample(input: SshRunInput) {
  return invoke<SshRunResult>("run_ssh_sample", { input });
}
