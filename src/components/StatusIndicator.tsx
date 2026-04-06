import type { AppStatus } from "../lib/tauri";

const STATUS_CONFIG = {
  disconnected: { color: "#555", label: "Disconnected", pulse: false },
  connecting: { color: "#f59e0b", label: "Connecting", pulse: true },
  mounted: { color: "#8BC53F", label: "Mounted", pulse: false },
  error: { color: "#f87171", label: "Error", pulse: true },
} as const;

export function StatusIndicator({ status }: { status: AppStatus }) {
  const config = STATUS_CONFIG[status.mount_status];

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <div
        style={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          backgroundColor: config.color,
          boxShadow: config.pulse ? undefined : `0 0 6px ${config.color}60`,
          animation: config.pulse ? "pulse 1.5s ease-in-out infinite" : "none",
        }}
      />
      <span
        style={{
          fontSize: 12,
          fontWeight: 500,
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
