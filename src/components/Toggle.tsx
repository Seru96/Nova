interface Props {
  label: string;
  checked: boolean;
  disabled?: boolean;
  tone?: "sky" | "amber";
  onChange: (on: boolean) => void;
}

export function Toggle({ label, checked, disabled, tone = "sky", onChange }: Props) {
  const on = tone === "amber" ? "bg-amber-500" : "bg-sky-500";
  return (
    <label className={`flex cursor-pointer select-none items-center gap-2 ${disabled ? "opacity-50" : ""}`}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative h-5 w-9 rounded-full transition-colors ${checked ? on : "bg-zinc-400/50"}`}
      >
        <span
          className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${checked ? "left-[18px]" : "left-0.5"}`}
        />
      </button>
      <span>{label}</span>
    </label>
  );
}
