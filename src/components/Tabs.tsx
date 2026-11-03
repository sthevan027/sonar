import { useRef, type KeyboardEvent, type ReactNode } from "react";

interface TabsProps {
  items: { id: string; label: string; icon: ReactNode }[];
  active: string;
  onChange: (id: string) => void;
}

/**
 * Abas com foco "roving": só a aba ativa entra na ordem de Tab (tabIndex 0); as outras
 * ficam em -1 e se alcançam com as setas, Home e End. Setas e Home/End também selecionam.
 */
export function Tabs({ items, active, onChange }: TabsProps) {
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const tabbableId = items.some((item) => item.id === active) ? active : items[0]?.id;

  function select(index: number) {
    const item = items[index];
    if (!item) return;
    onChange(item.id);
    buttons.current[index]?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const count = items.length;
    let target: number;
    switch (event.key) {
      case "ArrowRight":
        target = (index + 1) % count;
        break;
      case "ArrowLeft":
        target = (index - 1 + count) % count;
        break;
      case "Home":
        target = 0;
        break;
      case "End":
        target = count - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    select(target);
  }

  return (
    <div className="tabs" role="tablist">
      {items.map((item, index) => {
        const on = item.id === active;
        return (
          <button
            key={item.id}
            ref={(el) => {
              buttons.current[index] = el;
            }}
            type="button"
            role="tab"
            aria-selected={on}
            tabIndex={item.id === tabbableId ? 0 : -1}
            className={on ? "tab on" : "tab"}
            onClick={() => onChange(item.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            {item.icon}
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
