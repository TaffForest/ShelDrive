import { useState } from "react";

interface SettingsProps {
  onClose: () => void;
}

export function Settings({ onClose }: SettingsProps) {
  const [nodeUrl, setNodeUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [mountPoint, setMountPoint] = useState("/Volumes/ShelDrive");
  const [cacheSize, setCacheSize] = useState("512");

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
        <span
          style={{
            fontSize: 13,
            fontWeight: 600,
            letterSpacing: "0.05em",
            color: "var(--text-primary)",
          }}
        >
          SETTINGS
        </span>
        <button
          onClick={onClose}
          style={{
            fontSize: 11,
            color: "var(--text-secondary)",
            padding: "4px 10px",
            border: "1px solid var(--border)",
          }}
        >
          BACK
        </button>
      </div>

      {/* Fields */}
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
        <SettingsField
          label="SHELBY NODE URL"
          value={nodeUrl}
          onChange={setNodeUrl}
          placeholder="https://api.shelbynet.shelby.xyz/shelby"
        />
        <SettingsField
          label="API KEY"
          value={apiKey}
          onChange={setApiKey}
          placeholder="AG-..."
          type="password"
        />
        <SettingsField
          label="MOUNT POINT"
          value={mountPoint}
          onChange={setMountPoint}
          placeholder="/Volumes/ShelDrive"
        />
        <SettingsField
          label="CACHE SIZE (MB)"
          value={cacheSize}
          onChange={setCacheSize}
          placeholder="512"
        />

        <div style={{ marginTop: 8 }}>
          <button
            style={{
              width: "100%",
              padding: "10px 0",
              fontSize: 12,
              fontWeight: 600,
              letterSpacing: "0.05em",
              border: "1px solid var(--accent)",
              color: "var(--bg-primary)",
              backgroundColor: "var(--accent)",
            }}
          >
            SAVE
          </button>
          <div
            style={{
              fontSize: 10,
              color: "var(--text-muted)",
              marginTop: 8,
              textAlign: "center",
            }}
          >
            Changes saved to ~/.sheldrive/config.toml
          </div>
        </div>
      </div>
    </div>
  );
}

function SettingsField({
  label,
  value,
  onChange,
  placeholder,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  type?: string;
}) {
  return (
    <div>
      <div
        style={{
          fontSize: 10,
          color: "var(--text-muted)",
          marginBottom: 6,
          letterSpacing: "0.08em",
        }}
      >
        {label}
      </div>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        style={{
          width: "100%",
          padding: "8px 10px",
          fontSize: 12,
          fontFamily: "inherit",
          backgroundColor: "var(--bg-secondary)",
          border: "1px solid var(--border)",
          color: "var(--text-primary)",
          outline: "none",
          boxSizing: "border-box",
        }}
      />
    </div>
  );
}
