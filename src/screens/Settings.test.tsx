// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { ToastProvider } from "../components/Toast";
import type {
  AppState,
  Binding,
  BindingStatusEntry,
  Settings as AppSettings,
} from "../types";
import { Bindings } from "./Bindings";
import { BindingEditor } from "./BindingEditor";
import { Settings } from "./Settings";
import { ACTION_CHOICES } from "./bindingsModel";
import { shortcutFromKeys, shortcutKeys } from "./settingsModel";

const fx = vi.hoisted(() => ({ state: null as unknown as AppState }));

vi.mock("../store", () => ({
  useAppState: () => fx.state,
}));

vi.mock("../api", () => ({
  getSettings: vi.fn(),
  saveSettings: vi.fn(async () => undefined),
  pendingUpdate: vi.fn(async () => null),
  onUpdateAvailable: vi.fn(async () => () => {}),
  onUpdateProgress: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(async () => null),
  installUpdate: vi.fn(async () => undefined),
  getBindingStatus: vi.fn(async () => []),
  getDefaultBindings: vi.fn(async () => []),
  saveBindings: vi.fn(async () => []),
  onBindingsStatus: vi.fn(async () => () => {}),
  onActionResult: vi.fn(async () => () => {}),
  listRunningApps: vi.fn(async () => []),
}));

function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return { shortcut: "Super+Alt+V", autostart: false, ...overrides };
}

function setup(initial: AppSettings = settings()) {
  vi.mocked(api.getSettings).mockResolvedValue(initial);
  fx.state = {
    devices: [],
    sessions: [],
    rules: [],
    profiles: [],
    settings: initial,
    routing_available: true,
    last_profile: null,
    save_error: null,
  };
  const handlers = {
    onBack: vi.fn(),
    onOpenDevices: vi.fn(),
    onOpenProfiles: vi.fn(),
    onOpenBindings: vi.fn(),
  };
  render(
    <ToastProvider>
      <Settings {...handlers} />
    </ToastProvider>,
  );
  return handlers;
}

const PANEL_BINDING: Binding = { accelerator: "Super+Alt+V", action: { type: "toggle_panel" } };
const MUTE_BINDING: Binding = { accelerator: "Ctrl+Alt+M", action: { type: "toggle_output_mute" } };

/** Monta a tela de atalhos globais com uma lista de bindings e o estado de registro dela. */
function setupBindings(bindings: Binding[], statuses: BindingStatusEntry[] = []) {
  vi.mocked(api.getBindingStatus).mockResolvedValue(statuses);
  fx.state = {
    devices: [],
    sessions: [],
    rules: [],
    profiles: [{ id: "p1", icon: "layers", name: "Jogos", items: [] }],
    settings: { shortcut: "Super+Alt+V", autostart: false, bindings },
    routing_available: true,
    last_profile: null,
    save_error: null,
  };
  const handlers = { onBack: vi.fn() };
  render(
    <ToastProvider>
      <Bindings {...handlers} />
    </ToastProvider>,
  );
  return handlers;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Settings", () => {
  it("toggle_autostart_calls_save_settings", async () => {
    setup();
    const toggle = await screen.findByRole("switch", { name: "Iniciar com o Windows" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith({ shortcut: "Super+Alt+V", autostart: true }),
    );
    expect(
      screen.getByRole("switch", { name: "Iniciar com o Windows" }).getAttribute("aria-checked"),
    ).toBe("true");
  });

  it("toggle_autostart_nao_reenvia_a_lista_de_atalhos", async () => {
    // I3: reenviar `bindings` do snapshot desfaria uma edição ainda em voo no Rust.
    setup(settings({ bindings: [PANEL_BINDING, MUTE_BINDING] }));
    fireEvent.click(await screen.findByRole("switch", { name: "Iniciar com o Windows" }));

    await waitFor(() => expect(api.saveSettings).toHaveBeenCalled());
    const enviado = vi.mocked(api.saveSettings).mock.calls[0][0];
    expect("bindings" in enviado).toBe(false);
    expect(enviado).toEqual({ shortcut: "Super+Alt+V", autostart: true });
  });

  it("shortcut_capture_rejects_key_without_modifier_then_saves_combo", async () => {
    setup();
    const keys = await screen.findByRole("button", { name: /Atalho global: Win Alt V/ });
    fireEvent.click(keys);

    fireEvent.keyDown(keys, { key: "k", code: "KeyK" });
    const hint = screen.getByText("Inclua Ctrl, Alt ou Win.");
    // A dica é anunciada por leitor de tela (região viva, educada).
    expect(hint.getAttribute("aria-live")).toBe("polite");
    expect(api.saveSettings).not.toHaveBeenCalled();

    // Shift sozinho não conta como modificador de atalho global.
    fireEvent.keyDown(keys, { key: "k", code: "KeyK", shiftKey: true });
    expect(screen.getByText("Inclua Ctrl, Alt ou Win.")).toBeTruthy();
    expect(api.saveSettings).not.toHaveBeenCalled();

    fireEvent.keyDown(keys, { key: "k", code: "KeyK", ctrlKey: true });
    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith({ shortcut: "Ctrl+K", autostart: false }),
    );
  });

  it("double_toggle_ends_with_last_value_in_order", async () => {
    setup();
    const toggle = await screen.findByRole("switch", { name: "Iniciar com o Windows" });
    fireEvent.click(toggle);
    fireEvent.click(toggle);

    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledTimes(2));
    // Cada gravação leva o pedido vigente; a última fica com o valor final (desligado).
    const calls = vi.mocked(api.saveSettings).mock.calls;
    expect(calls[calls.length - 1][0].autostart).toBe(false);
    expect(
      screen.getByRole("switch", { name: "Iniciar com o Windows" }).getAttribute("aria-checked"),
    ).toBe("false");
  });

  it("failure_rolls_back_only_the_failed_field", async () => {
    setup();
    // A primeira gravação (ligar o autostart) falha; a segunda (atalho) passa.
    vi.mocked(api.saveSettings).mockRejectedValueOnce(new Error("disco cheio"));
    fireEvent.click(await screen.findByRole("switch", { name: "Iniciar com o Windows" }));
    const keys = screen.getByRole("button", { name: /Atalho global/ });
    fireEvent.click(keys);
    fireEvent.keyDown(keys, { key: "k", code: "KeyK", ctrlKey: true });

    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenLastCalledWith({ shortcut: "Ctrl+K", autostart: false }),
    );
    expect(await screen.findByText("Não foi possível salvar a configuração.")).toBeTruthy();
    expect(
      screen.getByRole("switch", { name: "Iniciar com o Windows" }).getAttribute("aria-checked"),
    ).toBe("false");
    expect(screen.getByRole("button", { name: "Atalho global: Ctrl K. Toque para trocar." })).toBeTruthy();
  });

  it("restore_default_shortcut_link_saves_the_default_from_the_backend", async () => {
    // O atalho de fábrica vem do Rust (`get_default_bindings`): a tela não guarda cópia.
    vi.mocked(api.getDefaultBindings).mockResolvedValue([
      { accelerator: "Ctrl+Alt+B", action: { type: "toggle_panel" } },
    ]);
    setup(settings({ shortcut: "Ctrl+K" }));
    fireEvent.click(await screen.findByRole("button", { name: "Restaurar atalho padrão (Ctrl Alt B)" }));
    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith({ shortcut: "Ctrl+Alt+B", autostart: false }),
    );
  });

  it("restore_default_shortcut_link_is_hidden_until_the_defaults_arrive", async () => {
    vi.mocked(api.getDefaultBindings).mockReturnValue(new Promise(() => {}));
    setup(settings({ shortcut: "Ctrl+K" }));
    await screen.findByRole("switch", { name: "Iniciar com o Windows" });
    expect(screen.queryByRole("button", { name: /Restaurar atalho padrão/ })).toBeNull();
  });

  it("shows_toast_when_save_settings_fails", async () => {
    setup();
    vi.mocked(api.saveSettings).mockRejectedValueOnce(new Error("disco cheio"));
    fireEvent.click(await screen.findByRole("switch", { name: "Iniciar com o Windows" }));
    expect(await screen.findByText("Não foi possível salvar a configuração.")).toBeTruthy();
  });

  it("shows_the_reason_when_the_shortcut_is_refused", async () => {
    setup();
    vi.mocked(api.saveSettings).mockRejectedValueOnce(
      "Não foi possível registrar o atalho Ctrl+K. Outro programa pode estar usando essa combinação.",
    );
    await screen.findByRole("switch", { name: "Iniciar com o Windows" });
    const keys = screen.getByRole("button", { name: /Atalho global/ });
    fireEvent.click(keys);
    fireEvent.keyDown(keys, { key: "k", code: "KeyK", ctrlKey: true });
    expect(await screen.findByText(/Outro programa pode estar usando/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Atalho global: Win Alt V. Toque para trocar." })).toBeTruthy();
  });

  it("shortcut_helpers_map_keys_and_validate_combo", () => {
    expect(shortcutKeys("Super+Alt+V")).toEqual(["Win", "Alt", "V"]);
    const none = { altKey: false, shiftKey: false, metaKey: false, ctrlKey: false };
    expect(shortcutFromKeys({ code: "ControlLeft", ...none, ctrlKey: true })).toEqual({
      status: "wait",
    });
    expect(
      shortcutFromKeys({ code: "Digit7", ctrlKey: false, altKey: true, shiftKey: true, metaKey: true }),
    ).toEqual({ status: "ok", shortcut: "Alt+Shift+Super+7" });
    expect(shortcutFromKeys({ code: "ArrowLeft", ...none, ctrlKey: true })).toMatchObject({
      status: "invalid",
    });
  });

  it("shortcut_capture_accepts_up_and_down_arrows", () => {
    const none = { altKey: false, shiftKey: false, metaKey: false, ctrlKey: false };
    expect(shortcutFromKeys({ code: "ArrowUp", ...none, ctrlKey: true, altKey: true })).toEqual({
      status: "ok",
      shortcut: "Ctrl+Alt+Up",
    });
    expect(shortcutFromKeys({ code: "ArrowDown", ...none, ctrlKey: true, altKey: true })).toEqual({
      status: "ok",
      shortcut: "Ctrl+Alt+Down",
    });
    // As setas laterais continuam recusadas: só sobe e desce são teclas de atalho.
    expect(shortcutFromKeys({ code: "ArrowLeft", ...none, ctrlKey: true })).toMatchObject({
      status: "invalid",
    });
    expect(shortcutFromKeys({ code: "ArrowRight", ...none, ctrlKey: true })).toMatchObject({
      status: "invalid",
    });
  });

  it("shortcut_capture_can_produce_every_combination_it_allows", () => {
    // Mesmo conjunto do `mainKeyOf`: 26 letras + 10 dígitos + 12 teclas F + as duas setas.
    const codes = [
      ...[..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"].map((letter) => `Key${letter}`),
      ...[..."0123456789"].map((digit) => `Digit${digit}`),
      ...Array.from({ length: 12 }, (_, i) => `F${i + 1}`),
      "ArrowUp",
      "ArrowDown",
    ];
    const modifiers = ["ctrlKey", "altKey", "shiftKey", "metaKey"] as const;
    const shortcuts = new Set<string>();
    for (let mask = 0; mask < 16; mask += 1) {
      const chosen = modifiers.filter((_, i) => (mask & (1 << i)) !== 0);
      // Shift sozinho não conta: sem Ctrl, Alt ou Win não há atalho global.
      if (!chosen.some((m) => m !== "shiftKey")) continue;
      for (const code of codes) {
        const outcome = shortcutFromKeys({
          code,
          ctrlKey: chosen.includes("ctrlKey"),
          altKey: chosen.includes("altKey"),
          shiftKey: chosen.includes("shiftKey"),
          metaKey: chosen.includes("metaKey"),
        });
        expect(outcome.status).toBe("ok");
        if (outcome.status === "ok") shortcuts.add(outcome.shortcut);
      }
    }
    // 14 combinações de modificadores (16 máscaras menos a vazia e a de só Shift) x 50 teclas.
    expect(codes.length).toBe(50);
    expect(shortcuts.size).toBe(700);
  });

  it("abre_a_tela_de_atalhos_globais", async () => {
    const handlers = setup();
    fireEvent.click(await screen.findByRole("button", { name: /Atalhos globais/ }));
    expect(handlers.onOpenBindings).toHaveBeenCalled();
  });
});

describe("Atalhos globais", () => {
  it("lista_os_bindings_com_titulo_detalhe_e_teclas", async () => {
    setupBindings([PANEL_BINDING, MUTE_BINDING]);
    expect(await screen.findByText("Atalho global")).toBeTruthy();
    expect(screen.getByText("Abre o painel na hora")).toBeTruthy();
    expect(screen.getByText("Mudo da saída padrão")).toBeTruthy();
    expect(screen.getByText("Alternar o mudo")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Atalho: Ctrl Alt M/ })).toBeTruthy();
  });

  it("binding_indisponivel_mostra_o_motivo_na_propria_linha", async () => {
    setupBindings([PANEL_BINDING, MUTE_BINDING], [
      {
        accelerator: "Ctrl+Alt+M",
        status: { type: "unavailable", message: "já em uso por outro aplicativo" },
      },
    ]);
    expect(await screen.findByText("Indisponível: já em uso por outro aplicativo")).toBeTruthy();
    // Os outros bindings continuam valendo (RF-17): nada trava a lista inteira.
    expect(screen.getByText("Abre o painel na hora")).toBeTruthy();
  });

  it("lista_vazia_mostra_a_mensagem_e_mantem_os_botoes", async () => {
    setupBindings([]);
    expect(await screen.findByText(/Nenhum atalho configurado/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Adicionar atalho" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sugerir atalhos de perfil" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Restaurar padrões" })).toBeTruthy();
  });

  it("atalho_duplicado_trava_o_salvar_e_mostra_a_mensagem", async () => {
    setupBindings([PANEL_BINDING, MUTE_BINDING]);
    await screen.findByText("Atalho global");
    fireEvent.click(screen.getAllByRole("button", { name: "Editar atalho" })[1]);
    const keys = await screen.findByRole("button", { name: /Atalho: Ctrl Alt M/ });
    fireEvent.click(keys);
    // A mesma combinação do primeiro binding ("Super+Alt+V"), com os modificadores em
    // outra ordem: o Rust compara os atalhos parseados, então é a mesma tecla.
    fireEvent.keyDown(keys, { key: "v", code: "KeyV", altKey: true, metaKey: true });
    expect(await screen.findByText("Este atalho já está em uso por outro atalho.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Salvar atalho" }).hasAttribute("disabled")).toBe(true);
  });

  it("recusa_do_backend_aparece_no_editor_e_nada_e_gravado", async () => {
    setupBindings([PANEL_BINDING, MUTE_BINDING]);
    vi.mocked(api.saveBindings).mockRejectedValueOnce(
      "Este atalho já está em uso por outro aplicativo.",
    );
    await screen.findByText("Atalho global");
    fireEvent.click(screen.getAllByRole("button", { name: "Editar atalho" })[1]);
    const keys = await screen.findByRole("button", { name: /Atalho: Ctrl Alt M/ });
    fireEvent.click(keys);
    fireEvent.keyDown(keys, { key: "k", code: "KeyK", ctrlKey: true, altKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Salvar atalho" }));
    expect(await screen.findByText("Este atalho já está em uso por outro aplicativo.")).toBeTruthy();
    // O editor continua aberto: a lista não foi gravada.
    expect(screen.getByRole("button", { name: "Salvar atalho" })).toBeTruthy();
  });

  it("remover_chama_save_bindings_com_a_lista_sem_o_item", async () => {
    setupBindings([PANEL_BINDING, MUTE_BINDING]);
    await screen.findByText("Atalho global");
    fireEvent.click(screen.getAllByRole("button", { name: "Editar atalho" })[1]);
    fireEvent.click(await screen.findByRole("button", { name: "Remover atalho" }));
    fireEvent.click(screen.getByRole("button", { name: "Remover" }));
    await waitFor(() => expect(api.saveBindings).toHaveBeenCalledWith([PANEL_BINDING]));
  });

  it("restaurar_padroes_busca_no_backend_e_substitui_a_lista", async () => {
    setupBindings([PANEL_BINDING]);
    // Os padrões vêm do Rust (Ruling 42): a tela não guarda cópia deles.
    const defaults: Binding[] = [
      { accelerator: "Super+Alt+V", action: { type: "toggle_panel" } },
      { accelerator: "Ctrl+Alt+M", action: { type: "toggle_output_mute" } },
    ];
    vi.mocked(api.getDefaultBindings).mockResolvedValue(defaults);

    fireEvent.click(await screen.findByRole("button", { name: "Restaurar padrões" }));
    fireEvent.click(screen.getByRole("button", { name: "Restaurar" }));

    await waitFor(() => expect(api.getDefaultBindings).toHaveBeenCalled());
    await waitFor(() => expect(api.saveBindings).toHaveBeenCalledWith(defaults));
  });

  it("sugerir_com_duplo_clique_grava_uma_vez_so", async () => {
    setupBindings([PANEL_BINDING]);
    // A primeira gravação fica em voo: a segunda não pode partir da lista antiga.
    vi.mocked(api.saveBindings).mockImplementationOnce(() => new Promise(() => undefined));
    const suggest = await screen.findByRole("button", { name: "Sugerir atalhos de perfil" });

    fireEvent.click(suggest);
    fireEvent.click(suggest);

    expect(api.saveBindings).toHaveBeenCalledTimes(1);
    expect(suggest.hasAttribute("disabled")).toBe(true);
  });

  it("a_tela_de_atalhos_cancela_as_assinaturas_ao_fechar", async () => {
    const stopStatus = vi.fn();
    vi.mocked(api.onBindingsStatus).mockResolvedValue(stopStatus);
    setupBindings([PANEL_BINDING]);
    await screen.findByText("Atalho global");
    await waitFor(() => expect(api.onBindingsStatus).toHaveBeenCalled());

    cleanup();

    await waitFor(() => expect(stopStatus).toHaveBeenCalledTimes(1));
  });

  it("a_tela_de_atalhos_nao_ouve_action_result_sozinha", async () => {
    // O aviso de falha é do App (vale em qualquer tela); aqui seria um listener a mais.
    setupBindings([PANEL_BINDING]);
    await screen.findByText("Atalho global");
    expect(api.onActionResult).not.toHaveBeenCalled();
  });

  it("seletor_de_acao_lista_ciclar_perfil_e_ciclar_saida_padrao", async () => {
    setupBindings([]);
    fireEvent.click(await screen.findByRole("button", { name: "Adicionar atalho" }));
    expect(await screen.findByRole("button", { name: /Próximo \/ Perfil anterior/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Próxima saída padrão/ })).toBeTruthy();
    expect(ACTION_CHOICES.map((c) => c.kind)).toEqual(
      expect.arrayContaining(["cycle_profile", "cycle_default_output"]),
    );
  });

  it("editor_de_parametro_abre_para_mudo_de_app", async () => {
    setupBindings([]);
    fireEvent.click(await screen.findByRole("button", { name: "Adicionar atalho" }));
    fireEvent.click(screen.getByRole("button", { name: /Mudo de um app/ }));
    expect(await screen.findByText("App")).toBeTruthy();
  });

  it("editor_de_parametro_abre_para_mover_app", async () => {
    setupBindings([]);
    fireEvent.click(await screen.findByRole("button", { name: "Adicionar atalho" }));
    fireEvent.click(screen.getByRole("button", { name: /Mover um app para outra saída/ }));
    expect(await screen.findByText("App")).toBeTruthy();
    expect(screen.getByText("Saída")).toBeTruthy();
  });

  it("editor_de_parametro_abre_para_ativar_perfil", async () => {
    setupBindings([]);
    fireEvent.click(await screen.findByRole("button", { name: "Adicionar atalho" }));
    fireEvent.click(screen.getByRole("button", { name: /Ativar um perfil/ }));
    expect(await screen.findByText("Perfil")).toBeTruthy();
  });
});

describe("BindingEditor: erro do backend (M12)", () => {
  afterEach(cleanup);

  it("avisa que o erro deixou de valer quando a ação muda", () => {
    const onEdit = vi.fn();
    render(
      <BindingEditor
        initial={{ accelerator: "Ctrl+Alt+K", action: { type: "toggle_panel" } }}
        others={[]}
        profiles={[]}
        devices={[]}
        error="Não foi possível registrar o atalho."
        onSave={() => {}}
        onRemove={() => {}}
        onCancel={() => {}}
        onEdit={onEdit}
      />,
    );
    // Abrir a tela não conta como edição.
    expect(onEdit).not.toHaveBeenCalled();

    const outra = ACTION_CHOICES.find((c) => c.kind !== "toggle_panel")!;
    fireEvent.click(screen.getByText(outra.label));

    expect(onEdit).toHaveBeenCalled();
  });
});
