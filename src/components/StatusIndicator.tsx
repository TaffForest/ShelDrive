import type { AppStatus } from "../lib/tauri";

const STATUS_CONFIG = {
  disconnected: { color: "#555555", label: "DISCONNECTED", pulse: false },
  connecting: { color: "#ffaa00", label: "CONNECTING", pulse: true },
  mounted: { color: "#00FFB2", label: "MOUNTED", pulse: false },
  error: { color: "#ff4444", label: "ERROR", pulse: true },
} as const;

export function StatusIndicator({ status }: { status: AppStatus }) {
  const config = STATUS_CONFIG[status.mount_status];

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
      <div
        style={{
          width: 10,
          height: 10,
          borderRadius: "50%",
          backgroundColor: config.color,
          boxShadow: `0 0 8px ${config.color}80`,
          animation: config.pulse ? "pulse 1.5s ease-in-out infinite" : "none",
        }}
      />
      <span
        style={{
          fontSize: 11,
          fontWeight: 600,
          letterSpacing: "0.1em",
          color: config.color,
        }}
      >
        {config.label}
      </span>
      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.3; }
        }
      `}</style>
    </div>
  );
}
