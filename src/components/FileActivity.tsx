import { useState, useEffect, useCallback } from "react";

interface ActivityEntry {
  id: number;
  operation: "pin" | "retrieve" | "unpin" | "error";
  filename: string;
  status: "pending" | "complete" | "error";
  timestamp: number;
  detail?: string;
}

const OP_STYLE = {
  pin: { color: "#00FFB2", symbol: "\u2191" },      // ↑
  retrieve: { color: "#66bbff", symbol: "\u2193" },  // ↓
  unpin: { color: "#888888", symbol: "\u00d7" },     // ×
  error: { color: "#ff4444", symbol: "!" },
} as const;

// Global activity log — persists across re-renders
let activityLog: ActivityEntry[] = [];
let nextId = 1;

export function pushActivity(
  operation: ActivityEntry["operation"],
  filename: string,
  status: ActivityEntry["status"],
  detail?: string,
) {
  activityLog = [
    {
      id: nextId++,
      operation,
      filename,
      status,
      timestamp: Date.now(),
      detail,
    },
    ...activityLog,
  ].slice(0, 50); // Keep last 50
}

export function FileActivity() {
  const [entries, setEntries] = useState<ActivityEntry[]>([]);

  const refresh = useCallback(() => {
    setEntries([...activityLog]);
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 2000);
    return () => clearInterval(interval);
  }, [refresh]);

  if (entries.length === 0) {
    return (
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
          ACTIVITY
        </div>
        <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
          No recent activity
        </div>
      </div>
    );
  }

  return (
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
        ACTIVITY
      </div>
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: 6,
          maxHeight: 120,
          overflowY: "auto",
        }}
      >
        {entries.slice(0, 8).map((entry) => {
          const op = OP_STYLE[entry.operation];
          const ago = formatAgo(entry.timestamp);
          return (
            <div
              key={entry.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                fontSize: 11,
              }}
            >
              <span style={{ color: op.color, width: 12, textAlign: "center" }}>
                {op.symbol}
              </span>
              <span
                style={{
                  flex: 1,
                  color: "var(--text-primary)",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {entry.filename}
              </span>
              <span style={{ color: "var(--text-muted)", fontSize: 10 }}>
                {ago}
              </span>
              {entry.status === "pending" && (
                <span
                  style={{
                    width: 6,
                    height: 6,
                    borderRadius: "50%",
                    backgroundColor: "#ffaa00",
                    animation: "pulse 1.5s ease-in-out infinite",
                  }}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function formatAgo(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000);
  if (seconds < 5) return "now";
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h`;
}
