import { useEffect, useState } from "react";
import * as api from "../api";
import { AppLogo } from "../components/AppLogo";
import { Card } from "../components/Card";
import { DeviceMenu } from "../components/DeviceMenu";
import { Icon } from "../components/Icon";
import { VolumeSlider } from "../components/VolumeSlider";
import type { AppState } from "../types";
import {
  appName,
  groupSessions,
  iconNameOf,
  quiet,
  routeFor,
  toneOf,
} from "./model";

/** Segmentos do medidor, de -60 dB a 0 dB. */
export const METER_SEGMENTS = 24;

/** Pico linear (0 a 1) em decibéis, com piso em -60 dB. */
export function peakToDb(peak: number): number {
  if (!(peak > 0)) return -60;
  return Math.max(-60, 20 * Math.log10(peak));
}

/** Quantos segmentos acender para um nível em dB. */
export function litSegments(db: number): number {
  const fraction = (Math.min(0, Math.max(-60, db)) + 60) / 60;
  return Math.round(fraction * METER_SEGMENTS);
}

/** Aba Microfone (mockup A2): mic padrão com medidor, volume e silenciar; apps usando o mic. */
export function MicTab({ state }: { state: AppState }) {
  const mic = state.devices.find((d) => d.flow === "capture" && d.is_default);
  const inputs = state.devices.filter((d) => d.flow === "capture" && d.connected && !d.hidden);
  const { active } = groupSessions(state.sessions, "capture");
  const micId = mic?.id ?? null;
  const [peak, setPeak] = useState(0);

  // Medidor: assina `mic-level` só enquanto esta aba está na tela; cancela ao sair.
  useEffect(() => {
    if (micId === null) return;
    let alive = true;
    let unlisten: (() => void) | undefined;
    api
      .onMicLevel((level) => {
        if (alive && level.id === micId) setPeak(level.peak);
      })
      .then((stop) => {
        if (alive) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
      setPeak(0);
    };
  }, [micId]);

  const db = peakToDb(peak);
  const lit = litSegments(db);

  return (
    <div className="tab-body">
      <Card>
        <div className="row">
          <div className="who">
            <div className="lbl">Microfone padrão</div>
            <b>{mic?.display_name ?? "Nenhum microfone conectado"}</b>
            <span className="o">{mic?.windows_name ?? "Conecte um microfone"}</span>
          </div>
          {mic && (
            <div className={toneOf(mic) === "brass" ? "tile hs" : "tile sg"}>
              <Icon name={iconNameOf(mic) ?? "mic"} size={20} />
            </div>
          )}
        </div>

        {mic && (
          <>
            <div className="row">
              <Icon name="audio-lines" size={18} />
              <div className="meter" aria-hidden="true">
                {Array.from({ length: METER_SEGMENTS }, (_, i) => (
                  <i key={i} className={i < lit ? "l" : undefined} />
                ))}
              </div>
              <span className="cnt mono meter-db">{Math.round(db)} dB</span>
            </div>

            <VolumeSlider
              label="Volume do microfone padrão"
              tone="brass"
              icon={<Icon name="mic" size={18} />}
              value={mic.volume}
              onCommit={(value) => api.setDeviceVolume(mic.id, value)}
            />

            <button
              type="button"
              className={mic.muted ? "mute off" : "mute"}
              onClick={() => quiet(api.setDeviceMuted(mic.id, !mic.muted))}
            >
              <Icon name="mic" size={18} />
              {mic.muted
                ? "Microfone silenciado · toque para ligar"
                : "Microfone ligado · toque para silenciar"}
            </button>
          </>
        )}
      </Card>

      <div className="section-head">
        <span className="lbl">Apps usando o microfone</span>
        <span className="cnt">
          <span className="dot" /> {active.length} agora
        </span>
      </div>

      <div className="app-list">
        {active.length === 0 && (
          <p className="empty">Nenhum app usando o microfone agora.</p>
        )}
        {active.map((row) => {
          const name = appName(row.exe);
          const route = routeFor(state, row.exe, "capture");
          const routed = state.devices.find((d) => d.id === route.selectedId);
          const chipIcon = iconNameOf(routed);
          return (
            <Card key={row.key}>
              <div className="row">
                <AppLogo exe={row.exe} />
                <div className="who">
                  <b>{name}</b>
                  <span className="o">{route.warning ?? "usando o microfone"}</span>
                </div>
                <DeviceMenu
                  kind="mic"
                  tone={toneOf(routed)}
                  label={route.label}
                  icon={chipIcon ? <Icon name={chipIcon} size={15} /> : undefined}
                  menuLabel={`Microfone de ${name}`}
                  devices={inputs}
                  selectedId={route.selectedId}
                  onSelect={(id) => quiet(api.setAppRoute(row.exe, "capture", id))}
                />
              </div>
            </Card>
          );
        })}
      </div>

      <div className="foot">
        <Icon name="info" size={15} />
        <span>O volume do mic é do dispositivo; cada app escolhe o seu.</span>
      </div>
    </div>
  );
}
