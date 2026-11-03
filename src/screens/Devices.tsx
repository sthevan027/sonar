import { useState } from "react";
import * as api from "../api";
import { Card } from "../components/Card";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { Tabs } from "../components/Tabs";
import { useToast } from "../components/Toast";
import type { DeviceTab } from "../routes";
import { useAppState } from "../store";
import type { DeviceView } from "../types";
import { DeviceCard } from "./DeviceCard";
import { DEVICE_FOOT, devicesOfFlow, iconOf } from "./devicesModel";
import { attempt } from "./model";
import { METER_SEGMENTS, litSegments, peakToDb } from "./MicTab";
import { useMicPeak } from "./useMicPeak";

interface DevicesProps {
  tab: DeviceTab;
  onTabChange: (tab: DeviceTab) => void;
  onBack: () => void;
}

/** Dispositivos de áudio (mockups D e D2): abas Saída e Entrada, cartões, padrão do Windows. */
export function Devices({ tab, onTabChange, onBack }: DevicesProps) {
  const state = useAppState();
  const notify = useToast();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);
  const isMic = tab === "capture";
  const list = devicesOfFlow(state.devices, tab);
  const connected = list.filter((d) => d.connected);
  const current = list.find((d) => d.is_default);

  function changeTab(id: string) {
    setEditingId(null);
    setSwitching(false);
    onTabChange(id === "capture" ? "capture" : "render");
  }

  function chooseDefault(device: DeviceView) {
    setSwitching(false);
    attempt(api.setDefaultDevice(device.id, tab), () =>
      notify(isMic ? "Não foi possível trocar o microfone padrão." : "Não foi possível trocar a saída padrão."),
    );
  }

  return (
    <div className="tab-body">
      <div className="row ed-top">
        <IconButton
          label="Voltar"
          icon={<Icon name="arrow-left" size={18} />}
          size={34}
          onPress={onBack}
        />
        <b className="ed-title">Dispositivos de áudio</b>
      </div>

      <Tabs
        items={[
          { id: "render", label: "Saída", icon: <Icon name="volume-2" size={17} /> },
          { id: "capture", label: "Entrada", icon: <Icon name="mic" size={17} /> },
        ]}
        active={tab}
        onChange={changeTab}
      />

      <Card>
        <div className="row">
          <div className="who">
            <span className="lbl">
              {isMic ? "Entrada padrão do Windows" : "Saída padrão do Windows"}
            </span>
            <b>
              {current?.display_name ??
                (isMic ? "Nenhum microfone conectado" : "Nenhuma saída conectada")}
            </b>
          </div>
          {connected.length > 1 && (
            <button
              type="button"
              className="btn sec"
              aria-expanded={switching}
              onClick={() => setSwitching((open) => !open)}
            >
              Trocar
            </button>
          )}
        </div>
        {switching && (
          <div
            role="radiogroup"
            aria-label={isMic ? "Entrada padrão" : "Saída padrão"}
            className="dev-pick"
          >
            {connected.map((device) => (
              <button
                key={device.id}
                type="button"
                role="radio"
                aria-checked={device.is_default}
                className={device.is_default ? "pick on" : "pick"}
                onClick={() => chooseDefault(device)}
              >
                <Icon name={iconOf(device)} size={16} />
                <span>{device.display_name}</span>
                {device.is_default && <Icon name="check" size={15} />}
              </button>
            ))}
          </div>
        )}
      </Card>

      <div className="app-list">
        {list.length === 0 && (
          <p className="empty">
            {isMic
              ? "Nenhum dispositivo de entrada encontrado."
              : "Nenhum dispositivo de saída encontrado."}
          </p>
        )}
        {list.map((device) => (
          <DeviceCard
            key={device.id}
            device={device}
            editing={editingId === device.id}
            onToggleEdit={() => setEditingId((id) => (id === device.id ? null : device.id))}
            footer={
              isMic && device.is_default && device.connected ? (
                <MicMeter id={device.id} />
              ) : undefined
            }
          />
        ))}
      </div>

      <div className="foot">
        <Icon name="info" size={15} />
        <span>{DEVICE_FOOT[tab]}</span>
      </div>
    </div>
  );
}

/** Medidor do mic padrão dentro do cartão (D2). Assina `mic-level` só enquanto está montado. */
function MicMeter({ id }: { id: string }) {
  const peak = useMicPeak(id);
  const lit = litSegments(peakToDb(peak));
  return (
    <div className="slider-row">
      <Icon name="audio-lines" size={16} />
      <div className="meter" aria-hidden="true">
        {Array.from({ length: METER_SEGMENTS }, (_, i) => (
          <i key={i} className={i < lit ? "l" : undefined} />
        ))}
      </div>
    </div>
  );
}
