import type { AppStatus } from "../lib/tauri";

interface MountControlsProps {
  status: AppStatus;
  loading: boolean;
  onMount: () => void;
  onUnmount: () => void;
}

export function MountControls({
  status,
  loading,
  onMount,
  onUnmount,
}: MountControlsProps) {
  const isMounted = status.mount_status === "mounted";
  const isConnecting = status.mount_status === "connecting";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", gap: 10 }}>
        <button
          onClick={onMount}
          disabled={isMounted || isConnecting || loading}
          style={{
            flex: 1,
            padding: "11px 0",
            fontSize: 14,
            fontWeight: 600,
            borderRadius: 6,
            border: "none",
            color: isMounted || loading ? "var(--text-muted)" : "#050505",
            backgroundColor:
              isMounted || loading ? "var(--bg-surface)" : "var(--accent)",
            transition: "all 0.15s ease",
          }}
        >
          {isConnecting ? "Connecting..." : "Mount"}
        </button>
        <button
          onClick={onUnmount}
          disabled={!isMounted || loading}
          style={{
            flex: 1,
            padding: "11px 0",
            fontSize: 14,
            fontWeight: 500,
            borderRadius: 6,
            border: "1px solid var(--border)",
            color: !isMounted || loading ? "var(--text-dim)" : "var(--text)",
            backgroundColor: "transparent",
            transition: "all 0.15s ease",
          }}
        >
          Unmount
        </button>
      </div>

      {status.error_message && (
        <div
          style={{
            fontSize: 12,
            color: "var(--error)",
            padding: "10px 12px",
            backgroundColor: "rgba(248, 113, 113, 0.06)",
            border: "1px solid rgba(248, 113, 113, 0.15)",
            borderRadius: 8,
          }}
        >
          {status.error_message}
        </div>
      )}
    </div>
  );
}
