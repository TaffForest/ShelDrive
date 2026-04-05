import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  mount_status: "disconnected" | "connecting" | "mounted" | "error";
  mount_point: string;
  error_message: string | null;
}

export async function getStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("get_status");
}

export async function mountDrive(): Promise<AppStatus> {
  return invoke<AppStatus>("mount_drive");
}

export async function unmountDrive(): Promise<AppStatus> {
  return invoke<AppStatus>("unmount_drive");
}

export async function getFileCount(): Promise<number> {
  return invoke<number>("get_file_count");
}

export interface ShelbyStatus {
  connected: boolean;
  network: string;
  node_url: string | null;
}

export async function getShelbyStatus(): Promise<ShelbyStatus> {
  return invoke<ShelbyStatus>("get_shelby_status");
}

export async function shelbyPing(): Promise<boolean> {
  return invoke<boolean>("shelby_ping");
}
