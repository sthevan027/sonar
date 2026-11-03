import { useState } from "react";
import * as api from "../api";
import { AppLogo } from "../components/AppLogo";
import { Card } from "../components/Card";
import { DeviceMenu } from "../components/DeviceMenu";
import { Icon } from "../components/Icon";
import { useToast } from "../components/Toast";
import { VolumeSlider } from "../components/VolumeSlider";
import type { AppState } from "../types";
import {
  appName,
  attempt,
  groupSessions,
  iconNameOf,
  routeFor,
  selectableDevices,
  toneOf,
  type AppRow,
} from "./model";

// Reexportado para a Task 12 e para os testes.
export { groupSessions, type AppRow } from "./model";

/** Aba Saída (mockup A): saída padrão, apps com som e a linha recolhida "Em silêncio". */
export function OutputTab({ state }: { state: AppState }) {
  const [silentOpen, setSilentOpen] = useState(false);
  const notify = useToast();
  const outputs = state.devices.filter((d) => d.flow === "render" && d.connected && !d.hidden);
  const defaultDevice = state.devices.find((d) => d.flow === "render" && d.is_default);
  const { active, silent } = groupSessions(state.sessions, "render");
  const nothingPlaying = active.length === 0 && silent.length === 0;

  return (
    <div className="tab-body">
      <Card>
        <div className="row">
          <div className="who">
            <div className="lbl">Saída padrão</div>
            <b>{defaultDevice?.display_name ?? "Nenhuma saída conectada"}</b>
            <span className="o">
              {defaultDevice?.windows_name ??
                (outputs.length > 0 ? "Escolha uma saída ao lado" : "Conecte um fone ou alto-falante")}
            </span>
          </div>
          {outputs.length > 0 && (
            <div role="radiogroup" aria-label="Saída padrão" className="tabs tabs-sm">
              {outputs.map((device) => (
                <button
                  key={device.id}
                  type="button"
                  role="radio"
                  aria-checked={device.is_default}
                  aria-label={device.display_name}
                  title={device.display_name}
                  className={device.is_default ? "tab tab-icon on" : "tab tab-icon"}
                  onClick={() =>
                    attempt(api.setDefaultDevice(device.id, "render"), () =>
                      notify("Não foi possível trocar a saída padrão."),
                    )
                  }
                >
                  <Icon name={iconNameOf(device) ?? "speaker"} size={18} />
                </button>
              ))}
            </div>
          )}
        </div>
        {defaultDevice && (
          <VolumeSlider
            label="Volume da saída padrão"
            tone="brass"
            icon={<Icon name="volume-2" size={18} />}
            value={defaultDevice.volume}
            onCommit={(value) => api.setDeviceVolume(defaultDevice.id, value)}
          />
        )}
      </Card>

      <div className="section-head">
        <span className="lbl">Apps com som</span>
        <span className="cnt">
          <span className="dot" /> {active.length} tocando
        </span>
      </div>

      <div className="app-list">
        {nothingPlaying && (
          <p className="empty">
            Nenhum app com som agora. Os apps aparecem aqui quando emitem som.
          </p>
        )}
        {!nothingPlaying && active.length === 0 && (
          <p className="empty">Nenhum app tocando agora.</p>
        )}
        {active.map((row) => (
          <AppCard key={row.key} row={row} state={state} />
        ))}

        {silent.length > 0 && (
          <>
            <button
              type="button"
              className="silent-toggle"
              aria-expanded={silentOpen}
              onClick={() => setSilentOpen((open) => !open)}
            >
              <Icon name={silentOpen ? "chevron-down" : "chevron-right"} size={15} />
              <span>Em silêncio</span>
              <span className="cnt">{silent.length}</span>
            </button>
            {silentOpen &&
              silent.map((row) => <AppCard key={row.key} row={row} state={state} />)}
          </>
        )}
      </div>

      {state.rules.length > 0 && (
        <div className="foot">
          <span>
            {state.rules.length === 1
              ? "1 regra salva. Reaplica ao abrir o app."
              : `${state.rules.length} regras salvas. Reaplicam ao abrir o app.`}
          </span>
        </div>
      )}
    </div>
  );
}

function AppCard({ row, state }: { row: AppRow; state: AppState }) {
  const notify = useToast();
  const name = appName(row.exe);
  const route = routeFor(state, row.exe, "render");
  const routed = state.devices.find((d) => d.id === route.selectedId);
  const chipIcon = iconNameOf(routed);
  const status = row.active ? "tocando agora" : "em silêncio";

  return (
    <Card tone={row.active ? "default" : "off"}>
      <div className="row">
        <AppLogo exe={row.exe} />
        <div className="who">
          <b>{name}</b>
          <span className="o">{route.warning ?? status}</span>
        </div>
        <DeviceMenu
          kind="device"
          tone={toneOf(routed)}
          label={route.label}
          icon={chipIcon ? <Icon name={chipIcon} size={15} /> : undefined}
          menuLabel={`Saída de ${name}`}
          devices={selectableDevices(state.devices, "render", route.selectedId)}
          selectedId={route.selectedId}
          onSelect={(id) =>
            attempt(api.setAppRoute(row.exe, "render", id), () =>
              notify(`Não foi possível mudar a saída de ${name}.`),
            )
          }
        />
      </div>
      <VolumeSlider
        label={`Volume do ${name}`}
        icon={<Icon name="volume-2" size={18} />}
        value={row.volume}
        onCommit={async (value) => {
          // Um PID que já fechou falha sem motivo: só é erro se todos falharem.
          const results = await Promise.allSettled(
            row.pids.map((pid) => api.setSessionVolume(pid, "render", value)),
          );
          if (results.length > 0 && results.every((r) => r.status === "rejected")) {
            throw new Error("volume do app");
          }
        }}
      />
    </Card>
  );
}
