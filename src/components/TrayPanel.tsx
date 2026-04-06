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
        background: "var(--bg-dark)",
      }}
    >
      {/* Header */}
      <div
        style={{
          padding: "14px 18px",
          borderBottom: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 16, fontWeight: 700 }}>
            Shel<span style={{ color: "var(--accent)" }}>Drive</span>
          </span>
          <span
            style={{
              fontSize: 10,
              color: "var(--text-dim)",
              fontWeight: 400,
              fontFamily: "'JetBrains Mono', monospace",
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
          padding: "16px 18px",
          display: "flex",
          flexDirection: "column",
          gap: 14,
          overflowY: "auto",
        }}
      >
        {/* Drive card */}
        <div
          style={{
            padding: "20px 18px",
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            borderRadius: 12,
          }}
        >
          <div
            className="mono"
            style={{
              fontSize: 20,
              fontWeight: 700,
              color:
                status.mount_status === "mounted"
                  ? "var(--accent)"
                  : "var(--text-dim)",
              letterSpacing: "-0.3px",
              marginBottom: 6,
            }}
          >
            {status.mount_point}
          </div>
          <div style={{ fontSize: 13, color: "var(--text-muted)" }}>
            {status.mount_status === "mounted"
              ? "Shelby network storage mounted"
              : "Click Mount to connect"}
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
            padding: "16px 18px",
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            borderRadius: 12,
          }}
        >
          <div
            style={{
              fontSize: 12,
              fontWeight: 600,
              color: "var(--text-muted)",
              marginBottom: 10,
            }}
          >
            Network
          </div>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 7,
              fontSize: 13,
            }}
          >
            <Row label="Protocol" value="SHELBYNET" mono />
            <Row
              label="Sidecar"
              value={shelby.connected ? "Connected" : "Mock mode"}
              accent={shelby.connected}
            />
            <Row label="Pinned files" value={String(fileCount)} mono />
          </div>
        </div>

        {/* Activity */}
        <FileActivity />
      </div>

      {/* Footer */}
      <div
        style={{
          padding: "10px 18px",
          borderTop: "1px solid var(--border)",
          fontSize: 12,
          color: "var(--text-dim)",
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <span>Shelby Protocol</span>
        <button
          onClick={() => setView("settings")}
          style={{
            fontSize: 12,
            color: "var(--text-muted)",
            padding: "4px 12px",
            border: "1px solid var(--border)",
            borderRadius: 6,
            transition: "all 0.15s",
          }}
        >
          Settings
        </button>
      </div>
    </div>
  );
}

function Row({
  label,
  value,
  mono,
  accent,
}: {
  label: string;
  value: string;
  mono?: boolean;
  accent?: boolean;
}) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between" }}>
      <span style={{ color: "var(--text-muted)" }}>{label}</span>
      <span
        className={mono ? "mono" : undefined}
        style={{
          color: accent ? "var(--accent)" : "var(--text)",
          fontSize: mono ? 12 : 13,
        }}
      >
        {value}
      </span>
    </div>
  );
}
