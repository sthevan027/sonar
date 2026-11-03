import type { ReactNode } from "react";

interface TabsProps {
  items: { id: string; label: string; icon: ReactNode }[];
  active: string;
  onChange: (id: string) => void;
}

export function Tabs({ items, active, onChange }: TabsProps) {
  return (
    <div className="tabs" role="tablist">
      {items.map((item) => {
        const on = item.id === active;
        return (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={on}
            className={on ? "tab on" : "tab"}
            onClick={() => onChange(item.id)}
          >
            {item.icon}
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
