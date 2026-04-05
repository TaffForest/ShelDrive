import { useState, useEffect } from "react";
import { useMountStatus } from "../hooks/useMountStatus";
import { StatusIndicator } from "./StatusIndicator";
import { MountControls } from "./MountControls";
import { FileActivity } from "./FileActivity";
import { Settings } from "./Settings";
import { getFileCount, getShelbyStatus, type ShelbyStatus } from "../lib/tauri";

export function TrayPanel() {
  const { status, loading, mount, unmount } = useMountStatus();
  const [fileCount, setFileCount] = useState(0);
  const [view, setView] = useState<"main" | "settings">("main");
  const [shelby, setShelby] = useState<ShelbyStatus>({
    connected: false,
    network: "SHELBYNET",
    node_url: null,
  });

  useEffect(() => {
    const poll = async () => {
      try {
        const [count, sh] = await Promise.all([
          getFileCount(),
          getShelbyStatus(),
        ]);
        setFileCount(count);
        setShelby(sh);
      } catch {
        // Tauri not available in dev browser
      }
    };
    poll();
    const interval = setInterval(poll, 5000);
    return () => clearInterval(interval);
  }, []);

  if (view === "settings") {
    return <Settings onClose={() => setView("main")} />;
  }

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        backgroundColor: "var(--bg-primary)",
        border: "1px solid var(--border)",
      }}
    >
      {/* Header */}
      <div
        style={{
          padding: "16px 20px",
          borderBottom: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span
            style={{
              fontSize: 15,
              fontWeight: 700,
              letterSpacing: "0.05em",
              color: "var(--accent)",
            }}
          >
            SHELDRIVE
          </span>
          <span
            style={{
              fontSize: 10,
              color: "var(--text-muted)",
              fontWeight: 400,
            }}
          >
            v0.1.0
          </span>
        </div>
        <StatusIndicator status={status} />
      </div>

      {/* Main content */}
      <div
        style={{
          flex: 1,
          padding: "20px",
          display: "flex",
          flexDirection: "column",
          gap: 16,
          overflowY: "auto",
        }}
      >
        {/* Drive visualization */}
        <div
          style={{
            padding: "20px",
            backgroundColor: "var(--bg-secondary)",
            border: "1px solid var(--border)",
          }}
        >
          <div
            style={{
              fontSize: 11,
              color: "var(--text-muted)",
              marginBottom: 12,
              letterSpacing: "0.08em",
            }}
          >
            VIRTUAL DRIVE
          </div>
          <div
            style={{
              fontSize: 28,
              fontWeight: 700,
              color:
                status.mount_status === "mounted"
                  ? "var(--accent)"
                  : "var(--text-muted)",
              letterSpacing: "0.02em",
              marginBottom: 8,
            }}
          >
            S:\
          </div>
          <div
            style={{
              fontSize: 11,
              color: "var(--text-secondary)",
            }}
          >
            {status.mount_status === "mounted"
              ? "Shelby network storage mounted and accessible"
              : "Drive not mounted \u2014 click MOUNT to connect"}
          </div>
        </div>

        {/* Controls */}
        <MountControls
          status={status}
          loading={loading}
          onMount={mount}
          onUnmount={unmount}
        />

        {/* Network info */}
        <div
          style={{
            padding: "16px 20px",
            backgroundColor: "var(--bg-secondary)",
            border: "1px solid var(--border)",
          }}
        >
          <div
            style={{
              fontSize: 11,
              color: "var(--text-muted)",
              marginBottom: 10,
              letterSpacing: "0.08em",
            }}
          >
            NETWORK
          </div>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 6,
              fontSize: 11,
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span style={{ color: "var(--text-secondary)" }}>protocol</span>
              <span>SHELBYNET</span>
            </div>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span style={{ color: "var(--text-secondary)" }}>sidecar</span>
              <span
                style={{
                  color: shelby.connected
                    ? "var(--accent)"
                    : "var(--text-muted)",
                }}
              >
                {shelby.connected ? "connected" : "mock mode"}
              </span>
            </div>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span style={{ color: "var(--text-secondary)" }}>
                pinned files
              </span>
              <span>{fileCount}</span>
            </div>
          </div>
        </div>

        {/* Activity feed */}
        <FileActivity />
      </div>

      {/* Footer */}
      <div
        style={{
          padding: "12px 20px",
          borderTop: "1px solid var(--border)",
          fontSize: 10,
          color: "var(--text-muted)",
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <span>Shelby Protocol</span>
        <button
          onClick={() => setView("settings")}
          style={{
            fontSize: 10,
            color: "var(--text-secondary)",
            padding: "2px 8px",
            border: "1px solid var(--border)",
          }}
        >
          SETTINGS
        </button>
      </div>
    </div>
  );
}
