import { iconMap, isIconName } from "../icons";

/** Ícone por nome. Nome desconhecido ou vazio não desenha nada (não quebra a tela). */
export function Icon({ name, size = 18 }: { name: string | null | undefined; size?: number }) {
  if (!isIconName(name)) return null;
  const Cmp = iconMap[name];
  return <Cmp className="ic" size={size} aria-hidden="true" />;
}
