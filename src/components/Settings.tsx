import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface SettingsProps {
  onClose: () => void;
}

export function Settings({ onClose }: SettingsProps) {
  const [nodeUrl, setNodeUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [privateKey, setPrivateKey] = useState("");
  const [saved, setSaved] = useState(false);

  const handleSave = async () => {
    try {
      await invoke("save_config", {
        rpcUrl: nodeUrl || null,
        apiKey: apiKey || null,
        privateKey: privateKey || null,
      });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error("Save failed:", e);
    }
  };

  return (
    <div
      style={{
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-dark)",
      }}
    >
      <div
        style={{
          padding: "14px 18px",
          borderBottom: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
      >
        <span style={{ fontSize: 14, fontWeight: 600 }}>Settings</span>
        <button
          onClick={onClose}
          style={{
            fontSize: 13,
            color: "var(--text-muted)",
            padding: "5px 14px",
            border: "1px solid var(--border)",
            borderRadius: 6,
            transition: "all 0.15s",
          }}
        >
          Back
        </button>
      </div>

      <div
        style={{
          flex: 1,
          padding: "18px",
          display: "flex",
          flexDirection: "column",
          gap: 16,
          overflowY: "auto",
        }}
      >
        <Field
          label="Shelby RPC URL"
          value={nodeUrl}
          onChange={setNodeUrl}
          placeholder="https://api.testnet.shelby.xyz/shelby"
        />
        <Field
          label="API Key"
          value={apiKey}
          onChange={setApiKey}
          placeholder="AG-..."
          type="password"
        />
        <Field
          label="Private Key"
          value={privateKey}
          onChange={setPrivateKey}
          placeholder="0x..."
          type="password"
        />

        <div style={{ marginTop: 4 }}>
          <button
            onClick={handleSave}
            style={{
              width: "100%",
              padding: "11px 0",
              fontSize: 14,
              fontWeight: 600,
              borderRadius: 6,
              border: "none",
              color: "#050505",
              background: saved ? "var(--accent-dark, #6BA030)" : "var(--accent)",
              transition: "all 0.15s",
            }}
          >
            {saved ? "Saved" : "Save"}
          </button>
          <div
            style={{
              fontSize: 11,
              color: "var(--text-dim)",
              marginTop: 8,
              textAlign: "center",
            }}
          >
            Saves to ~/.sheldrive/config.toml — restart app to apply
          </div>
        </div>
      </div>
    </div>
  );
}

function Field({
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
          fontSize: 12,
          fontWeight: 500,
          color: "var(--text-muted)",
          marginBottom: 6,
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
          padding: "9px 12px",
          fontSize: 13,
          fontFamily: "'JetBrains Mono', monospace",
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          borderRadius: 8,
          color: "var(--text)",
          outline: "none",
          boxSizing: "border-box",
          transition: "border-color 0.15s",
        }}
      />
    </div>
  );
}
