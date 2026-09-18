export interface ViewSettings {
  signals: boolean; // synaptic signals: curved links + particles that fire on recall
  biolum: boolean; // curated bioluminescent palette
}

export const DEFAULT_VIEW: ViewSettings = {
  signals: true,
  biolum: true,
};

const LABELS: { key: keyof ViewSettings; label: string; title: string }[] = [
  { key: "signals", label: "signals", title: "Curved links; particles fire along bonds on recall" },
  { key: "biolum", label: "biolum", title: "Curated bioluminescent colour palette" },
];

interface Props {
  settings: ViewSettings;
  onToggle: (key: keyof ViewSettings) => void;
}

export function ViewToggles({ settings, onToggle }: Props) {
  return (
    <div className="view-toggles" role="group" aria-label="Visual effects">
      {LABELS.map(({ key, label, title }) => (
        <button
          key={key}
          className={`vt ${settings[key] ? "on" : ""}`}
          title={title}
          aria-pressed={settings[key]}
          onClick={() => onToggle(key)}
        >
          <span className="vt-dot" />
          {label}
        </button>
      ))}
    </div>
  );
}
