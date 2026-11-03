import { useEffect, useState } from "react";
import * as api from "../api";
import { AppLogo } from "../components/AppLogo";
import { useToast } from "../components/Toast";
import type { RunningApp } from "../types";
import { appFromPath, filterApps } from "./editorModel";

interface AppPickerProps {
  /** Executáveis que já estão no perfil: não aparecem na lista. */
  exclude: string[];
  onPick: (app: RunningApp) => void;
  onCancel: () => void;
}

/**
 * Seletor de app para o perfil: lista os apps com janela aberta, com busca, e "Procurar .exe"
 * para um app que ainda não está aberto. Fica dentro do editor (não é rota), para o rascunho sobreviver.
 */
export function AppPicker({ exclude, onPick, onCancel }: AppPickerProps) {
  const notify = useToast();
  const [apps, setApps] = useState<RunningApp[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");

  useEffect(() => {
    let alive = true;
    api.listRunningApps().then(
      (list) => {
        if (alive) setApps(list);
      },
      () => {
        if (alive) setFailed(true);
      },
    );
    return () => {
      alive = false;
    };
  }, []);

  const taken = new Set(exclude.map((exe) => exe.toLowerCase()));
  const visible = filterApps(
    (apps ?? []).filter((app) => !taken.has(app.exe.toLowerCase())),
    query,
  );

  function browse() {
    api.pickExecutable().then(
      (path) => {
        const app = path === null ? null : appFromPath(path);
        if (app) onPick(app);
      },
      () => notify("Não foi possível abrir o seletor de arquivos."),
    );
  }

  let body;
  if (failed) {
    body = <p className="ed-note">Não foi possível listar os apps abertos. Use Procurar .exe.</p>;
  } else if (apps === null) {
    body = <p className="ed-note">Procurando apps abertos…</p>;
  } else if (visible.length === 0) {
    body = (
      <p className="ed-note">
        {query.trim() ? "Nenhum app encontrado." : "Nenhum app aberto com janela."}
      </p>
    );
  } else {
    body = (
      <ul className="pick-list" aria-label="Apps abertos">
        {visible.map((app) => (
          <li key={app.exe}>
            <button type="button" className="pick" onClick={() => onPick(app)}>
              <AppLogo exe={app.exe} />
              <span>{app.name}</span>
            </button>
          </li>
        ))}
      </ul>
    );
  }

  return (
    <div className="picker">
      <div className="picker-top">
        <input
          type="search"
          className="ed-input"
          aria-label="Buscar app"
          placeholder="Buscar app"
          autoComplete="off"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <button type="button" className="btn" onClick={browse}>
          Procurar .exe
        </button>
      </div>
      {body}
      <button type="button" className="ed-link" onClick={onCancel}>
        Cancelar
      </button>
    </div>
  );
}
