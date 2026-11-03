import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
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
  /** Dispositivos oferecidos (quem chama já filtra com `selectableDevices`). */
  devices: DeviceView[];
  /** Id do dispositivo marcado; `null` = Padrão do Windows. */
  selectedId: string | null;
  onSelect: (id: string | null) => void;
}

interface Choice {
  id: string | null;
  label: string;
}

const GAP_PX = 6;

/**
 * Lado em que o menu abre. Para baixo se cabe; senão para cima, se houver mais espaço em
 * cima. Todas as medidas no mesmo referencial (viewport ou o contêiner de rolagem).
 */
export function chooseSide(m: {
  triggerTop: number;
  triggerBottom: number;
  menuHeight: number;
  boundsTop: number;
  boundsBottom: number;
}): "down" | "up" {
  const below = m.boundsBottom - m.triggerBottom - GAP_PX;
  const above = m.triggerTop - m.boundsTop - GAP_PX;
  if (below >= m.menuHeight) return "down";
  return above > below ? "up" : "down";
}

/**
 * Chip que abre um menu de escolha de dispositivo. Itens: "Padrão do Windows" (`null`) e um
 * por dispositivo. Teclado: seta para baixo no chip abre; setas, Home e End movem (foco
 * "roving": só o item ativo entra no Tab); Enter ou Espaço escolhem; Esc fecha de qualquer
 * ponto do menu e devolve o foco ao chip; Tab fecha e segue a ordem de foco normal.
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
  const [side, setSide] = useState<"down" | "up">("down");
  const [active, setActive] = useState(0);
  const wrapRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLUListElement>(null);
  // Guarda de ponteiro: com o botão do mouse pressionado dentro do menu, a perda de foco não
  // fecha o menu (um clique no respiro entre os itens tira o foco do item).
  const pointerInMenu = useRef(false);

  const choices: Choice[] = [
    { id: null, label: "Padrão do Windows" },
    ...devices.map((d) => ({
      id: d.id,
      label: d.hidden ? `${d.display_name} (oculto)` : d.display_name,
    })),
  ];
  const selectedIndex = Math.max(
    0,
    choices.findIndex((c) => c.id === selectedId),
  );

  // Ao abrir: mede se cabe para baixo, rola para deixar o menu visível e foca o item ativo.
  useLayoutEffect(() => {
    if (!open) return;
    const trigger = triggerRef.current;
    const menu = menuRef.current;
    if (trigger && menu) {
      const bounds = wrapRef.current?.closest(".app-list")?.getBoundingClientRect();
      const triggerRect = trigger.getBoundingClientRect();
      setSide(
        chooseSide({
          triggerTop: triggerRect.top,
          triggerBottom: triggerRect.bottom,
          menuHeight: menu.offsetHeight,
          boundsTop: bounds?.top ?? 0,
          boundsBottom: bounds?.bottom ?? window.innerHeight,
        }),
      );
      menu.scrollIntoView?.({ block: "nearest" });
    }
    menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]')[active]?.focus();
    // Só na abertura: `active` já foi definido por `openMenu`.
  }, [open]);

  // Solta a guarda de ponteiro quando o botão sobe, em qualquer lugar.
  useEffect(() => {
    if (!open) return;
    const release = () => {
      pointerInMenu.current = false;
    };
    document.addEventListener("pointerup", release);
    return () => document.removeEventListener("pointerup", release);
  }, [open]);

  function itemsOf(): HTMLElement[] {
    return Array.from(
      menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? [],
    );
  }

  function openMenu() {
    setActive(selectedIndex);
    setOpen(true);
  }

  function close(returnFocus: boolean) {
    setOpen(false);
    if (returnFocus) triggerRef.current?.focus();
  }

  function choose(id: string | null) {
    onSelect(id);
    close(true);
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (!open) {
      if (event.key === "ArrowDown" && event.target === triggerRef.current) {
        event.preventDefault();
        openMenu();
      }
      return;
    }
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        close(true);
        return;
      case "Tab":
        // Não cancela o Tab: o foco segue para o próximo controle a partir do chip.
        close(true);
        return;
      default:
        break;
    }

    const list = itemsOf();
    const count = list.length;
    if (count === 0) return;
    const current = list.indexOf(document.activeElement as HTMLElement);
    let next: number;
    switch (event.key) {
      case "ArrowDown":
        next = current < 0 ? 0 : (current + 1) % count;
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
    setActive(next);
    list[next].focus();
  }

  // Foco saindo do conjunto fecha o menu, exceto quando foi um clique dentro dele.
  function onBlur(event: FocusEvent<HTMLDivElement>) {
    if (pointerInMenu.current) return;
    const next = event.relatedTarget as Node | null;
    if (!next || !wrapRef.current?.contains(next)) setOpen(false);
  }

  function onMenuPointerDown() {
    pointerInMenu.current = true;
  }

  // Clique no respiro do menu (fora dos itens) não tira o foco de dentro dele.
  function onMenuMouseDown(event: MouseEvent<HTMLUListElement>) {
    const target = event.target as HTMLElement;
    if (!target.closest('[role="menuitemradio"]')) event.preventDefault();
  }

  return (
    <div className="dmenu" ref={wrapRef} onKeyDown={onKeyDown} onBlur={onBlur}>
      <Chip
        ref={triggerRef}
        kind={kind}
        tone={tone}
        label={label}
        icon={icon}
        expanded={open}
        onPress={() => (open ? close(false) : openMenu())}
      />
      {open && (
        <ul
          ref={menuRef}
          role="menu"
          aria-label={menuLabel}
          className={side === "up" ? "dmenu-list up" : "dmenu-list"}
          onPointerDown={onMenuPointerDown}
          onMouseDown={onMenuMouseDown}
        >
          {choices.map((choice, index) => (
            <MenuItem
              key={choice.id ?? "default"}
              label={choice.label}
              checked={choice.id === selectedId}
              tabIndex={index === active ? 0 : -1}
              onFocus={() => setActive(index)}
              onPress={() => choose(choice.id)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function MenuItem({
  label,
  checked,
  tabIndex,
  onFocus,
  onPress,
}: {
  label: string;
  checked: boolean;
  tabIndex: number;
  onFocus: () => void;
  onPress: () => void;
}) {
  return (
    <li role="none">
      <button
        type="button"
        role="menuitemradio"
        aria-checked={checked}
        tabIndex={tabIndex}
        className={checked ? "dmenu-item on" : "dmenu-item"}
        onFocus={onFocus}
        onClick={onPress}
      >
        <span>{label}</span>
        {checked && <Icon name="check" size={15} />}
      </button>
    </li>
  );
}
