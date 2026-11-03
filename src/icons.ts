// Nomes de ícone usados pela interface -> componente lucide-react. O mesmo conjunto dos
// mockups em docs/mockups/. Nomes vêm do estado (ex.: ícone de perfil) e podem não existir:
// use `isIconName` antes de confiar num texto.
import {
  ArrowLeft,
  AudioLines,
  Bluetooth,
  BookOpen,
  Briefcase,
  Check,
  ChevronDown,
  ChevronRight,
  Coffee,
  Eye,
  EyeOff,
  Gamepad2,
  Headphones,
  Info,
  Keyboard,
  Layers,
  Mic,
  Monitor,
  Music,
  Pencil,
  Plus,
  Power,
  Rocket,
  RotateCcw,
  Settings,
  Speaker,
  Tv,
  Video,
  Volume2,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

export const iconMap = {
  speaker: Speaker,
  headphones: Headphones,
  mic: Mic,
  monitor: Monitor,
  tv: Tv,
  bluetooth: Bluetooth,
  "gamepad-2": Gamepad2,
  briefcase: Briefcase,
  music: Music,
  "book-open": BookOpen,
  coffee: Coffee,
  rocket: Rocket,
  video: Video,
  eye: Eye,
  "eye-off": EyeOff,
  pencil: Pencil,
  x: X,
  plus: Plus,
  "chevron-down": ChevronDown,
  "chevron-right": ChevronRight,
  "arrow-left": ArrowLeft,
  "rotate-ccw": RotateCcw,
  info: Info,
  power: Power,
  settings: Settings,
  keyboard: Keyboard,
  layers: Layers,
  "audio-lines": AudioLines,
  "volume-2": Volume2,
  check: Check,
} satisfies Record<string, LucideIcon>;

export type IconName = keyof typeof iconMap;

export function isIconName(name: string | null | undefined): name is IconName {
  return typeof name === "string" && Object.prototype.hasOwnProperty.call(iconMap, name);
}
