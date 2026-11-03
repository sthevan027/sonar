import type { ReactNode } from "react";

interface IconButtonProps {
  /** Texto acessível: o botão não tem texto visível. */
  label: string;
  icon: ReactNode;
  onPress: () => void;
  /** Lado em px, como nos mockups: 32 (`.ab`), 34 (`.back`), 38 (`.g`) ou 44 (`.pbtn`). */
  size?: 32 | 34 | 38 | 44;
  selected?: boolean;
  disabled?: boolean;
}

export function IconButton({ label, icon, onPress, size = 32, selected, disabled }: IconButtonProps) {
  const className = ["ibtn", `s${size}`, selected ? "sel" : ""].filter(Boolean).join(" ");
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      className={className}
      onClick={onPress}
    >
      {icon}
    </button>
  );
}
