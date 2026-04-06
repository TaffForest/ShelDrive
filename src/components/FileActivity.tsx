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
  pin: { color: "#8BC53F", symbol: "\u2191" },
  retrieve: { color: "#7ec8e3", symbol: "\u2193" },
  unpin: { color: "#999", symbol: "\u00d7" },
  error: { color: "#f87171", symbol: "!" },
} as const;

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
  ].slice(0, 50);
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

  return (
    <div
      style={{
        background: "var(--bg-card)",
        border: "1px solid var(--border)",
        borderRadius: 12,
        padding: "16px 18px",
      }}
    >
      <div
        style={{
          fontSize: 12,
          fontWeight: 600,
          color: "var(--text-muted)",
          marginBottom: entries.length > 0 ? 12 : 0,
        }}
      >
        Activity
      </div>
      {entries.length === 0 ? (
        <div style={{ fontSize: 12, color: "var(--text-dim)", marginTop: 6 }}>
          No recent activity
        </div>
      ) : (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 6,
            maxHeight: 110,
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
                  fontSize: 12,
                }}
              >
                <span style={{ color: op.color, width: 14, textAlign: "center", fontWeight: 600 }}>
                  {op.symbol}
                </span>
                <span
                  className="mono"
                  style={{
                    flex: 1,
                    color: "var(--text)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontSize: 11,
                  }}
                >
                  {entry.filename}
                </span>
                <span style={{ color: "var(--text-dim)", fontSize: 11 }}>
                  {ago}
                </span>
                {entry.status === "pending" && (
                  <span
                    style={{
                      width: 6,
                      height: 6,
                      borderRadius: "50%",
                      backgroundColor: "var(--warning)",
                      animation: "pulse 1.5s ease-in-out infinite",
                    }}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}
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
