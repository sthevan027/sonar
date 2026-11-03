interface ToggleProps {
  on: boolean;
  onChange: (value: boolean) => void;
  /** Nome acessível do interruptor. */
  label?: string;
}

export function Toggle({ on, onChange, label }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className={on ? "tog" : "tog off"}
      onClick={() => onChange(!on)}
    >
      <i />
    </button>
  );
}
