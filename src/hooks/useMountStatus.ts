import { useState, useEffect, useCallback } from "react";
import { AppStatus, getStatus, mountDrive, unmountDrive } from "../lib/tauri";

export function useMountStatus() {
  const [status, setStatus] = useState<AppStatus>({
    mount_status: "disconnected",
    mount_point: "/Volumes/ShelDrive",
    error_message: null,
  });
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const s = await getStatus();
      setStatus(s);
    } catch {
      // Tauri not available (dev browser mode) — keep default
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 3000);
    return () => clearInterval(interval);
  }, [refresh]);

  const mount = useCallback(async () => {
    setLoading(true);
    try {
      const s = await mountDrive();
      setStatus(s);
    } catch {
      setStatus((prev) => ({
        ...prev,
        mount_status: "error",
        error_message: "Failed to mount drive",
      }));
    } finally {
      setLoading(false);
    }
  }, []);

  const unmount = useCallback(async () => {
    setLoading(true);
    try {
      const s = await unmountDrive();
      setStatus(s);
    } catch {
      setStatus((prev) => ({
        ...prev,
        mount_status: "error",
        error_message: "Failed to unmount drive",
      }));
    } finally {
      setLoading(false);
    }
  }, []);

  return { status, loading, mount, unmount, refresh };
}
