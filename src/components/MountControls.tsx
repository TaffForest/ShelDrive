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
      <div
        style={{
          display: "flex",
          gap: 8,
        }}
      >
        <button
          onClick={onMount}
          disabled={isMounted || isConnecting || loading}
          style={{
            flex: 1,
            padding: "10px 0",
            fontSize: 12,
            fontWeight: 600,
            letterSpacing: "0.05em",
            border: "1px solid var(--accent)",
            color: isMounted || loading ? "var(--text-muted)" : "var(--bg-primary)",
            backgroundColor:
              isMounted || loading ? "transparent" : "var(--accent)",
            transition: "all 0.15s ease",
          }}
        >
          {isConnecting ? "CONNECTING..." : "MOUNT"}
        </button>
        <button
          onClick={onUnmount}
          disabled={!isMounted || loading}
          style={{
            flex: 1,
            padding: "10px 0",
            fontSize: 12,
            fontWeight: 600,
            letterSpacing: "0.05em",
            border: "1px solid var(--border)",
            color:
              !isMounted || loading ? "var(--text-muted)" : "var(--text-primary)",
            backgroundColor: "transparent",
            transition: "all 0.15s ease",
          }}
        >
          UNMOUNT
        </button>
      </div>

      <div
        style={{
          fontSize: 11,
          color: "var(--text-secondary)",
          display: "flex",
          justifyContent: "space-between",
        }}
      >
        <span>mount point</span>
        <span style={{ color: "var(--text-primary)" }}>
          {status.mount_point}
        </span>
      </div>

      {status.error_message && (
        <div
          style={{
            fontSize: 11,
            color: "var(--error)",
            padding: "8px 10px",
            backgroundColor: "#ff444410",
            border: "1px solid #ff444430",
          }}
        >
          {status.error_message}
        </div>
      )}
    </div>
  );
}
