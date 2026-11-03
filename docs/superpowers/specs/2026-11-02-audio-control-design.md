# Sonar — design (v1)

> Nome do produto: **Sonar** **[decidido, 2026-11-02]**. O repositório ainda se chama `audio-control` (renomear está pendente). Ícone em estudo (3 versões no Doop, https://doop.design/c/QGNSVqwX0n; recomendação: V3 "S de rota"). Decisões do Sthevan estão marcadas como **[decidido]**; escolhas minhas estão marcadas como **[minha escolha]** para ele conferir. Contexto e histórico em `docs/DECISOES.md`.

## 1. Intenção e por quê

O controle de som do Windows 10/11 exige muitos cliques e não permite deixar, de forma simples, a música na caixa e o Discord/Chrome no headset. O Sthevan quer um app de bandeja, bonito e calmo, que:

1. Mande **cada app para uma saída** (e uma entrada de microfone) diferente e **lembre isso sozinho**.
2. Tenha **perfis** (ex.: "Jogo") que trocam várias rotas com um clique.
3. Deixe **renomear** e dar ícone aos dispositivos.

É ferramenta pessoal **e** produto: gratuito, aberto ou fechado ainda não decidido. Também serve de peça de portfólio (Rust + Tauri + Windows).

## 2. Objetivos, não-objetivos, anti-objetivos

**Objetivos da v1**
- Roteamento **por app** de saída **e** de entrada (microfone), com regras salvas e reaplicadas automaticamente.
- Volume e mute **por app** (saída); volume e mute do **dispositivo** (saída e microfone).
- Trocar a saída/entrada padrão do Windows pelo painel.
- Perfis com ícone, criados e editados pelo usuário, ativados **só manualmente**.
- Dispositivos de áudio com nome editável, ícone e opção de ocultar.
- Painel na bandeja (abre ao clicar), atalho global, iniciar com o Windows.
- Windows 11 (validado) e Windows 10 (implementado, **não testado**).

**Não-objetivos da v1** (não fazer, nem preparar além do necessário)
- Equalizador, efeitos, normalização (exigiria driver de áudio virtual).
- Volume de microfone **por app** (o Windows não oferece).
- Ativação automática de perfil (detectar jogo abrindo).
- Tela de "primeiro uso" (fica para depois, na construção).
- Tela de arrastar apps entre dispositivos (reprovada).
- Instalador assinado, loja de apps, atualização automática (pode vir depois).

**Anti-objetivos** (seria um fracasso mesmo se tecnicamente funcionar)
- Visual estressante ou carregado como o EarTrumpet; o painel tem que ser calmo e legível de relance.
- Mexer na configuração do Windows que o usuário não pediu (ex.: apps fora de um perfil devem ficar como estavam).
- Perder a configuração do usuário por um arquivo corrompido.

## 3. Decidido pelo Sthevan

- **Stack:** Tauri (Rust + React/TypeScript), a mesma do Focus Brew. App **separado** do Focus Brew. Gerenciador JS: **Bun**.
- **Painel abre ao clicar** no ícone da bandeja. Atalho global e iniciar com o Windows entram na v1.
- **Regras lembradas:** o app lembra a rota de cada app e reaplica sozinho. Se o dispositivo some, o app cai no padrão do Windows; **ao reconectar, volta sozinho**.
- **Perfis:** opcionais; só mexem nos apps que listam; os demais ficam como estavam. Ativação sempre manual. Identificados por **ícone**; nome opcional.
- **Persistência:** arquivo JSON (padrão do Focus Brew). SQLite só se crescer.
- **Dispositivos:** renomear, ícone por dispositivo e ocultar do painel (Q19 confirmada).
- **Microfone** entra na v1: qual app usa qual mic; perfis incluem mic.
- **Telas** (mockups no Doop, https://doop.design/c/2qgDviRoxZ): **A** painel/Saída, **A2** painel/Microfone, **C** editar perfil, **D** dispositivos de saída, **D2** dispositivos de entrada, **E** menu de configurações. Os mockups são a **referência visual vinculante** e estão no repositório como HTML autocontido em `docs/mockups/` (A, A2, C, D, D2, E; abrir no navegador, 420×700). Conteúdo e interações: ver `docs/DECISOES.md`, seção "Design do painel".
- **Visual:** "estúdio à noite" — grafite `#1B1A18`, latão `#D9A441` (destaque), azul-acinzentado `#86A9B8` (caixa), verde `#8FBF7A` (atividade); Bricolage Grotesque (títulos) + DM Sans (texto) + JetBrains Mono (números); ícones Lucide. Painel de ~388×668 px.

## 4. Fatos técnicos verificados (spike, Windows 11 build 26300)

- Roteamento por app: `RoGetActivationFactory("Windows.Media.Internal.AudioPolicyConfig")` com o IID da variante "21H2" (`ab3d4648-e242-459f-b02f-541c70306324`); métodos `SetPersistedDefaultAudioEndpoint`, `GetPersistedDefaultAudioEndpoint`, `ClearAllPersistedApplicationDefaultEndpoints` (vtable: 6 slots de IUnknown+IInspectable, 19 slots reservados, depois os três). Variante Downlevel (`2a59116d-6c4f-45e0-a74f-707e3fef9258`, 17 slots reservados) para builds < 21390 (limiar do EarTrumpet).
- ID de dispositivo enviado ao Windows: `\\?\SWD#MMDEVAPI#{endpoint-id}#{e6327cad-dcec-4949-ae8a-991e976a79d2}` (saída) ou `...#{2eef81be-33fa-4800-9670-1cd474972c3f}` (entrada). **Duas barras iniciais**; com uma só o Windows devolve `E_INVALIDARG` sem pista.
- Os dois papéis (`eMultimedia`=1 e `eConsole`=0) são gravados a cada rota; passar HSTRING vazio limpa a rota (volta ao padrão).
- Escrita de **entrada** aceita (`eCapture`), verificada no Discord.
- Código de referência: `src/main.rs` (spike) e os projetos EarTrumpet (MIT) e Earmark.

**Não verificado (testar antes de prometer):**
- A rota sobrevive a fechar/reabrir o app e a reiniciar o Windows? (a chamada recebe um PID, então provavelmente precisa ser reaplicada.)
- O Chrome usa um processo de áudio separado: o PID da sessão é o certo?
- Trocar entre **dois microfones reais** (só há um ativo no PC de casa).
- Comportamento do Windows quando o dispositivo roteado some.
- Windows 10 (variante Downlevel) e builds entre 19041 e 21389.

## 5. Arquitetura

**Backend Rust (`src-tauri`)**, módulos separados e testáveis:
- `audio/devices` — enumera dispositivos de saída/entrada (`IMMDeviceEnumerator`), nome amigável, estado, padrão do Windows; escuta adição/remoção (`IMMNotificationClient`).
- `audio/sessions` — enumera sessões de áudio por dispositivo (`IAudioSessionManager2`); escuta novas sessões (`IAudioSessionNotification`); volume/mute por sessão de saída (`ISimpleAudioVolume`).
- `audio/endpoint` — volume e mute do **dispositivo** (`IAudioEndpointVolume`), nível do microfone para o medidor.
- `audio/policy` — o roteador por app (`AudioPolicyConfig`): `set_route(pid, flow, device_id)`, `clear_route(pid, flow)`, `get_route`. Escolhe a variante pela build do Windows.
- `audio/default_device` — trocar a saída/entrada **padrão do Windows** via `IPolicyConfig` (também não documentado; o crate `com-policy-config` cobre isso, ou vtable própria). Define os **três papéis** (console, multimedia, communications) de uma vez. **[minha escolha]**
- Os módulos `audio/policy` e `audio/default_device` são os **únicos** que usam COM não documentado; ambos isolados atrás de traits para poder simular em testes.
- `rules` — regras `{exe, flow, device_id}`; ao surgir uma sessão ou um dispositivo voltar, reaplica.
- `profiles` — perfis `{id, ícone, nome?, itens: [{exe, saída?, entrada?}]}`; ativar = aplicar as rotas e atualizar as regras lembradas desses apps.
- `store` — leitura/gravação do JSON, **gravação atômica** (arquivo temporário + renomear) com cópia de segurança (o Focus Brew tem esse risco anotado).
- `shell` — bandeja, janela do painel (posicionada perto do ícone, fecha ao perder o foco), atalho global, instância única, iniciar com o Windows.

**Frontend (React + TypeScript, Bun, Vite):** telas A, A2, C, D, D2, E. Fala com o Rust por comandos e eventos do Tauri; o estado de áudio vem do Rust (fonte única da verdade). Ícones locais (nada de hotlink).

**Dados** em `directories::ProjectDirs` (como o Focus Brew): `settings.json` (atalho, iniciar com o Windows), `devices.json` (nome e ícone por ID de endpoint, oculto), `rules.json`, `profiles.json`. Todos versionados (campo `version`) e tolerantes a campos novos.

## 6. Comportamentos

- **Regra automática:** a chave é o **nome do executável** (minúsculo) + fluxo (saída/entrada). Ao surgir uma sessão de áudio de um app com regra, grava a rota para o PID dela.
- **Primeira escolha manual** de um app cria a regra.
- **Dispositivo some:** o Windows cai no padrão; a regra **não é apagada**; o dispositivo aparece esmaecido com "regras guardadas". Ao voltar, reaplica a todos os apps com regra para ele.
- **Perfil:** só mexe nos apps listados; não desfaz o que não lista.
- **Origem da lista de apps:** (1) sessões de áudio existentes no Windows (tocando ou **inativas**, que é o que a linha recolhida "Em silêncio" mostra); (2) no editor de perfil (tela C), "Adicionar app" lista os **processos em execução com janela** e oferece **Procurar .exe**. Nome amigável e ícone vêm do executável. Apps que nunca abriram e não estão rodando só entram por "Procurar .exe". **[minha escolha]** (a pergunta Q18 não foi respondida; escopo mínimo, sem varrer apps instalados.)
- **Chave da regra:** nome do executável (minúsculo) + fluxo. Vale para **todos os PIDs** daquele exe que tiverem sessão de áudio (cobre Chrome/Electron com vários processos, a confirmar no teste). Apps UWP/MSIX (identidade por pacote) **não** estão na v1. **[minha escolha]**
- **Reaplicação:** ao iniciar e a cada nova sessão/dispositivo, o app aplica as regras também a apps que já estavam rodando. Se o Windows tiver uma rota própria conflitante, **a regra do app vence**. **[minha escolha]**
- **Itens do perfil:** em `{exe, saída?, entrada?}`, campo **ausente = não mexe** naquela direção; valor explícito `"padrão"` = volta ao padrão do Windows (limpa a rota). Ativar um perfil **atualiza as regras lembradas** dos apps listados (o perfil "Jogo" deixa o Spotify na caixa até algo mudar isso); não há "desativar". **[minha escolha — o Sthevan deve conferir esta]**
- **Volume por app:** o Windows já guarda; o app **não** guarda por conta própria. **[minha escolha]**
- **Atalho global padrão:** `Win+Alt+V`, editável. **[minha escolha]**
- **Perfil ativo:** o painel mostra qual foi o último ativado; não há "desativar" (as regras ficam). **[minha escolha]**

## 7. Restrições e riscos

- A API de roteamento **não é documentada** e pode mudar entre builds do Windows. Isolada em `audio/policy`, com testes de "contrato" por build e mensagem clara se a chamada falhar (o app continua funcionando sem rotear).
- Não rodar projeto no `C:`; repositório em `D:\Projetos`.
- Sem telemetria nem rede na v1.

## 8. Testes

- Unitários (sem Windows): geração/limpeza do ID de dispositivo (111 caracteres, duas barras), regras, perfis, gravação atômica do JSON.
- Manuais no PC de casa (checklist): rotear Spotify↔Caixa/Headset; Discord no mic; reconectar o headset; fechar/reabrir apps; reiniciar o Windows; perfil "Jogo"; atalho global; iniciar com o Windows.
- Windows 10: checklist idêntico quando houver uma máquina para testar.

## 9. Deixado para quem for construir

Estrutura de pastas do repositório (o spike hoje está na raiz: mover para `spike/` e criar o app Tauri), versões das dependências, formato interno dos JSONs além do descrito, estratégia de estado no React, animações, texto de erro, acessibilidade (foco e teclado), empacotamento.

## 10. Em aberto (só o Sthevan decide)

- ~~Nome do produto~~ → **Sonar** (decidido). Falta checar conflito de marca/nome e escolher o ícone.
- **Licença**: código aberto ou fechado.
