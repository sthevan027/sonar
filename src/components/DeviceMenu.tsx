import { useEffect, useRef, useState, type FocusEvent, type KeyboardEvent, type ReactNode } from "react";
import type { DeviceView } from "../types";
import { Chip } from "./Chip";
import { Icon } from "./Icon";

interface DeviceMenuProps {
  kind: "device" | "mic";
  tone: "brass" | "sage";
  /** Texto do chip: o dispositivo escolhido ou "Padrão do Windows". */
  label: string;
  icon?: ReactNode;
  /** Nome acessível do menu, por exemplo "Saída do Spotify". */
  menuLabel: string;
  /** Dispositivos oferecidos (quem chama já filtra: conectados, do fluxo certo, não ocultos). */
  devices: DeviceView[];
  /** Id do dispositivo marcado; `null` = Padrão do Windows. */
  selectedId: string | null;
  onSelect: (id: string | null) => void;
}

/**
 * Chip que abre um menu de escolha de dispositivo. Itens: "Padrão do Windows" (`null`) e um
 * por dispositivo. Teclado: setas, Home e End movem; Enter/Espaço escolhem (botão nativo);
 * Esc fecha e devolve o foco ao chip; sair do menu (Tab ou clique fora) fecha.
 */
export function DeviceMenu({
  kind,
  tone,
  label,
  icon,
  menuLabel,
  devices,
  selectedId,
  onSelect,
}: DeviceMenuProps) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLUListElement>(null);

  // Ao abrir, o foco vai para a opção marcada (ou a primeira).
  useEffect(() => {
    if (!open) return;
    const menu = menuRef.current;
    const target =
      menu?.querySelector<HTMLElement>('[role="menuitemradio"][aria-checked="true"]') ??
      menu?.querySelector<HTMLElement>('[role="menuitemradio"]');
    target?.focus();
  }, [open]);

  function close(returnFocus: boolean) {
    setOpen(false);
    if (returnFocus) triggerRef.current?.focus();
  }

  function choose(id: string | null) {
    onSelect(id);
    close(true);
  }

  function items(): HTMLElement[] {
    return Array.from(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? []);
  }

  function onMenuKeyDown(event: KeyboardEvent<HTMLUListElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      close(true);
      return;
    }
    const list = items();
    const count = list.length;
    if (count === 0) return;
    const current = list.indexOf(document.activeElement as HTMLElement);
    let next: number;
    switch (event.key) {
      case "ArrowDown":
        next = (current + 1) % count;
        break;
      case "ArrowUp":
        next = current <= 0 ? count - 1 : current - 1;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = count - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    list[next].focus();
  }

  // Foco saindo do conjunto (Tab, clique em área sem foco) fecha o menu.
  function onBlur(event: FocusEvent<HTMLDivElement>) {
    const next = event.relatedTarget as Node | null;
    if (!next || !wrapRef.current?.contains(next)) setOpen(false);
  }

  return (
    <div className="dmenu" ref={wrapRef} onBlur={onBlur}>
      <Chip
        ref={triggerRef}
        kind={kind}
        tone={tone}
        label={label}
        icon={icon}
        expanded={open}
        onPress={() => setOpen((value) => !value)}
      />
      {open && (
        <ul
          ref={menuRef}
          role="menu"
          aria-label={menuLabel}
          className="dmenu-list"
          onKeyDown={onMenuKeyDown}
        >
          <MenuItem checked={selectedId === null} onPress={() => choose(null)}>
            Padrão do Windows
          </MenuItem>
          {devices.map((device) => (
            <MenuItem
              key={device.id}
              checked={device.id === selectedId}
              onPress={() => choose(device.id)}
            >
              {device.display_name}
            </MenuItem>
          ))}
        </ul>
      )}
    </div>
  );
}

function MenuItem({
  checked,
  onPress,
  children,
}: {
  checked: boolean;
  onPress: () => void;
  children: ReactNode;
}) {
  return (
    <li role="none">
      <button
        type="button"
        role="menuitemradio"
        aria-checked={checked}
        className={checked ? "dmenu-item on" : "dmenu-item"}
        onClick={onPress}
      >
        <span>{children}</span>
        {checked && <Icon name="check" size={15} />}
      </button>
    </li>
  );
}
