# Sonar: spec de desenvolvimento dos comandos globais (v1.1)

> **Status:** rascunho, aguardando aprovação da v1 (`superpowers/specs/2026-11-02-audio-control-design.md`) e do checklist manual (`VERIFICACAO.md`). Não começar a implementar antes dos bloqueios da §2.
> **Data:** 2026-11-04. **Base:** [sonar-system-design.md](sonar-system-design.md).
>
> Este documento diz **o que** construir, em que ordem e como verificar. O **porquê** e o detalhe ficam no design. Itens marcados **proposta** são defaults meus, para o autor confirmar. Itens **PENDENTE** dependem de spike ou de decisão.

---

## 1. Status e escopo

**Versão alvo: v1.1**, comandos globais sobre a v1 já especificada.

| Entra na v1.1 | Fonte |
|---|---|
| Camada de ações tipadas (`Action`) e dispatcher (fila + worker) | design §4, §5, §6 |
| Ações A2, A3, A4, A6, A7, A8, A9, A10 | design §4 |
| Bindings de atalho em Configurações, com migração do `shortcut` legado | design §7.2 |

| Não entra na v1.1 | Motivo |
|---|---|
| Equalizador e efeitos | Próxima versão (design §11.1). Spikes S3 a S6 seguem como base técnica, sem prazo |
| Detecção automática de perfil | Decisão do autor: ativação só manual |
| A11, mudo geral | Fora até haver desfazer (design §11, D5) |
| A5, mudo do microfone | Depende do spike S1 e da F4 do design |
| A12, reaplicar regras | Interno, não exposto |
| Volume de microfone por app | Não suportado pelo Windows |
| Entrada por linha de comando (`--action`) | Opção, não requisito (design §6) |

---

## 2. Pré-requisitos (bloqueios)

| # | Bloqueio | Critério de liberação |
|---|---|---|
| B1 | Checklist manual da v1 (`VERIFICACAO.md`) passado no Win11 build 26300 | Todos os itens marcados e falhas anotadas (seção "Após a verificação") |
| B2 | Spec v1 aprovada pelo autor | Status da spec v1 deixa de ser "aguardando aprovação" |
| B3 | Spike S4 (conflito de atalho) | Bloqueia o **registro dos padrões** (F1 e F2). Antes disso, usar atalhos de teste |

---

## 3. Comandos globais

Convenções: toda ação devolve `{ ok, changed, resolved, error_code, message }` (design §4). A ação de atalho **não abre pop-up**; erro vai para evento ou notificação, porque o painel pode estar oculto.

- **`ok` e `changed` são independentes.** `ok` = "a ação cumpriu o pedido inteiro"; `changed` = "o estado mudou de fato". Uma ação que cumpriu **só parte** do pedido devolve `ok: false` **e** `changed: true`, com o `error_code` e a `message` do primeiro erro — nunca `ok: true` com o erro escondido. `ok == false` **não** implica `changed == false`: o painel não pode descartar a atualização de estado só porque a ação falhou.
- **A `message` de uma falha parcial diz as duas coisas** (o que mudou e o que falhou), para o usuário não ler "erro" e achar que nada aconteceu.
- **`error_code`** é um de: `profile_not_found`, `no_profiles`, `no_output_device`, `not_enough_outputs`, `device_gone`, `unsupported`, `os_error`, `invalid_exe`, `invalid_value`, `queue_full`, `internal` (snake_case). `internal` é falha **do app** (ex.: pânico no executor), não recusa do Windows.
- **O evento `action-result`** é emitido quando `ok == false` — é por ele que o aviso chega ao usuário, inclusive nos casos de sucesso parcial.

| ID | Ação | Alvo | Atalho padrão | Comportamento esperado | Erro esperado |
|---|---|---|---|---|---|
| A2 | Ativar perfil `profile_id` | global | Sem atalho semeado. O atalho é criado pelo usuário, como binding de perfil. **Proposta (Task 16):** a tela de Configurações sugere `Ctrl+Alt+1`…`Ctrl+Alt+9` para os perfis que ainda não têm binding | Aplica `profiles::activate`: rotas e regras dos apps listados | "Perfil não encontrado." (design: `commands.rs:126`). Roteamento indisponível: erro com código |
| A3 | Próximo perfil (`direction: next \| prev`) | global | `Ctrl+Alt+P` (próximo, proposta) | Avança na ordem da lista de perfis, volta ao início no fim, e aplica (como A2) | Sem perfis: erro `no_profiles` (`ok: false`), com aviso ao usuário pelo evento `action-result`. Perfil atual apagado: vai ao primeiro |
| A4 | Mudo da saída padrão (`muted: true \| false`) | saída padrão (render) | `Ctrl+Alt+M` (ver Q1) | Define o mudo do endpoint padrão | "O dispositivo não está mais disponível." (`Gone`) |
| A6 | Volume da saída padrão, relativo (`delta`) | saída padrão | `Ctrl+Alt+↑` / `Ctrl+Alt+↓` (±0,05) — **provisionais até o S4** | Soma os deltas de uma rajada (coalescência) e aplica uma vez. Resultado com clamp 0 a 1 | Igual A4 |
| A7 | Volume absoluto da saída padrão (`value`) | saída padrão | Sem padrão (definir após S4) | Define o volume; clamp 0 a 1; a última ação vence. Ressalva: dentro do epsilon de 0,0001 do valor atual, nada é gravado e `changed` é `false` — mesma regra de idempotência de `set` do RF-06/RF-14 | Igual A4 |
| A8 | Mudo de app (`exe`, `muted`) | sessões de render do exe, todos os pids | Sem padrão (definir após S4) | Define o mudo de todas as sessões existentes do exe | Sem sessão ativa: `ok` com `changed: false`, não é erro. Exe inválido: erro de validação |
| A9 | Mover app (`exe`, `device` ou "Padrão") | regra do exe | Sem padrão (definir após S4) | Igual `set_app_route`: grava a regra e aplica. **Proposta (Task 15):** o literal `"Padrão"` (ou `"Default"`) é normalizado para a saída padrão do Windows (dispositivo ausente) | "Este Windows não permite essa operação." (`Unsupported`). Dispositivo ausente: regra guardada, sem erro |
| A10 | Próxima saída padrão (`direction`) | saídas conectadas e visíveis, na ordem do painel (proposta) | Sem padrão (definir após S4) | Troca o padrão para a próxima saída; volta à primeira no fim | Menos de 2 saídas: erro `not_enough_outputs` (`ok: false`), com aviso ao usuário pelo evento `action-result` |

**Proposta sobre A9:** entra na F2, porque precisa de parâmetros por binding. O design não fixa a fase.

**Ressalva sobre `routing_available`:** o campo `AppState.routing_available` é um **latch histórico** — ele fica `false` se alguma reconciliação esbarrou em `Unsupported` e só volta a `true` com um roteamento bem-sucedido. Ele **não** é o veredito de uma ação: a A2 decide pelo resultado do reconcile **daquela** ativação, para que uma falha antiga não produza um `unsupported` falso. A UI que mostrar esse campo precisa saber que ele pode estar `false` com o Windows aceitando roteamento.

---

## 4. Requisitos funcionais

Cada requisito tem um teste ou verificação manual indicado.

| ID | Requisito | Verificação |
|---|---|---|
| RF-01 | Toda entrada (atalho, bandeja, IPC) vira `Action` tipada (enum com tag `serde`). Tipo desconhecido gera erro, nunca é ignorado | Unitário: JSON com `type` inexistente é rejeitado |
| RF-02 | O handler do atalho só enfileira e retorna. A execução fica no worker, sob `Engine::apply` | Integração: fila cheia ou worker parado não bloqueia o produtor |
| RF-03 | A2 ativa um perfil existente. Perfil inexistente devolve `error_code` e não muda nada | Unitário: id inexistente gera `ok: false` e estado igual ao anterior |
| RF-04 | A3 avança pela ordem da lista e volta ao início | Unitário: 3 perfis, 4 chamadas, resultado 1, 2, 3, 1 |
| RF-05 | A3 com perfil atual apagado vai ao primeiro perfil | Unitário |
| RF-06 | A4 `set` define o mudo; se já está no estado pedido, `changed: false` e nada é gravado em disco | Unitário: `set(true)` duas vezes gera 1 mudança e 1 escrita |
| RF-07 | O atalho de mudo faz toggle resolvido sob `apply` (lê o estado atual) e chama `set` com o valor invertido | Unitário com fake + manual. Depende de Q1 |
| RF-08 | A6: uma rajada de deltas dentro da janela de coalescência (proposta: 50 a 100 ms) vira um ajuste só, com a soma. Resultado com clamp | Unitário: de 0,20 com 10 deltas de +0,05 chega a 0,70 em uma chamada |
| RF-09 | A7 é absoluta, com clamp e sem coalescência | Unitário |
| RF-10 | A8 afeta só as sessões que existem no momento (proposta: não é regra persistente) | Unitário com fake de sessões |
| RF-11 | A9 usa o mesmo caminho de `set_app_route` e guarda a regra quando o dispositivo está ausente | Unitário com fake de `RouteBackend` |
| RF-12 | A10 troca para a próxima saída conectada e visível. Com uma saída só, não faz nada | Unitário: 3 saídas, uma oculta, o ciclo passa pelas 2 visíveis |
| RF-13 | Atalhos funcionam com qualquer app em foco, sem rede e com o app de destino fechado (decisão do autor, design §11.1) | Manual (ver §9.3) |
| RF-14 | Ações `set` são idempotentes. Ações não idempotentes (A3, A6, A10) informam `changed` no resultado | Unitário |
| RF-15 | Trocar o atalho registra o **novo antes** de soltar o antigo. Se o registro do novo falhar, o antigo continua valendo (RF-17) | Manual + unitário com fake do plugin |
| RF-16 | Validação antes de gravar: sintaxe do atalho, duplicata dentro do app recusada, tamanho máximo (proposta: 64 caracteres) | Unitário |
| RF-17 | Troca falha: o atalho anterior continua valendo e a UI mostra o aviso. Na inicialização, binding que falha fica "indisponível" e os demais seguem | Manual (VERIFICACAO, "Atalho em uso") + unitário |
| RF-18 | Binding apontando para **perfil apagado** (`profile_not_found`) ou **exe inválido** (`invalid_exe`): a ação devolve erro e o binding é mantido (não é removido sozinho). Dispositivo ausente **não** é erro: a A9 guarda a regra e devolve `ok: true` (§3, A9) | Unitário |
| RF-19 | Migração: `shortcut` legado vira binding `toggle_panel` na primeira leitura. O arquivo antigo fica em `.bak` | Unitário com arquivo de v1 real (manual) |
| RF-20 | Os comandos IPC atuais (`set_*`) não mudam de contrato. O painel continua igual | Testes atuais + manual da F0 |

---

## 5. Requisitos não funcionais

| ID | Requisito | Como verificar |
|---|---|---|
| RNF-01 | Nenhum COM nem lock de dados é segurado no handler do atalho | Revisão de código + RF-02 |
| RNF-02 | Fila limitada (proposta: 64 ações). Fila cheia descarta com aviso e não trava. **Proposta (Task 8):** `Dispatcher::submit` devolve `SubmitError::Full` e todo ponto que chama `submit` trata o erro — `eprintln!` **e** aviso ao frontend (evento `action-result` com `ok: false`), para que o "aviso" exista de fato | Unitário, com o aviso chegando |
| RNF-03 | Ordem de locks preservada: `apply` > `persist` > `emit` > `inner` (design §2.3). Nenhum lock novo de dados | Revisão de código |
| RNF-04 | Idempotência: `set` repetido não gera escrita nem evento extra | RF-06 |
| RNF-05 | Dispositivo sumido: erro tratado com `error_code`, o app segue usável, sem `unwrap` no caminho da ação | Unitário com fake que devolve `Gone` + manual (ver §9.3) |
| RNF-06 | Contrato preservado: snapshot `AppState`, evento `state-changed`, contador `generation` (design §2.3). **Ressalva (proposta, Task 12):** `Settings.bindings` entra em `Settings`, que está dentro de `AppState` — o shape do snapshot muda por isso. O teste fixado `app_state_json_shape_is_pinned` é reescrito **de propósito**, com o campo novo escrito explicitamente e documentado no doc do teste | Testes atuais, mais o pin reescrito |
| RNF-07 | Sem rede e sem telemetria (design §7) | Revisão |
| RNF-08 | Mensagens de erro em português. Nada de código hexadecimal cru na mensagem do atalho (design §4) | Manual |
| RNF-09 | Latência de execução do atalho: **não medida**. Medir na F1 no Win11 e registrar aqui. Sem meta até medir | Medição manual na F1 |

---

## 6. Configuração de atalhos

Modelo: `Settings.bindings: [{ accelerator, action }]`. `shortcut` continua sendo lido só para migração (design §7.2).

| Tema | Regra |
|---|---|
| Migração | `shortcut` vira `{ accelerator: shortcut, action: toggle_panel }` na primeira leitura. **Proposta (Task 12):** a migração semeia também os atalhos padrão de mudo e volume (`Ctrl+Alt+M`, `Ctrl+Alt+↑`, `Ctrl+Alt+↓`), para o usuário não precisar recriá-los. `.bak` antes de gravar (`store.rs:49-68`, design §7.2) |
| Versão do Store | `CURRENT_VERSION` sobe de 1 para 2 (`store.rs:12`), com a migração condicionada à versão (design §7.3). **Proposta (Task 12):** a migração só roda uma vez — abrir o app de novo não regrava nem duplica bindings |
| Validação | Sintaxe, duplicata recusada, tamanho máximo (RF-16) |
| Troca | Registrar o novo **antes** de soltar o antigo. Hipótese resolvida: o código existente já faz nessa ordem (`shell.rs:258-271`) e o RF-17 exige que o anterior continue valendo enquanto o novo é registrado. **Proposta (Task 13):** a troca por binding (`apply_bindings`) segue a mesma ordem |
| Falha na troca | Mantém o anterior e mostra aviso (RF-17) |
| Falha na inicialização | Binding fica "indisponível" com mensagem; os outros seguem (RF-17) |
| Teclas reservadas pelo sistema | Hipótese de falha de registro (ex.: Win+L). **PENDENTE:** execução do S4 no Win11 build 26300. **Proposta (Task 9):** a sonda (registra e solta cada tecla, uma por vez) fica pronta antes do registro dos padrões, e os padrões seguem **provisionais** até o S4 |
| Interface de Configurações | Mockup E tem uma linha única "Atalho global". A lista de bindings, o estado "indisponível" e a edição de cada um têm desenho: rascunho local de 62 KB (`bindings-screen-draft.html`, fora do repo), com a decisão **D1** — a linha "Atalho global" **convive** com a lista nova e **é** o binding `toggle_panel`, o primeiro da lista. **PENDENTE:** publicar no Doop e obter a aprovação do autor antes da F2 (os mockups são vinculantes, design §3) |
| Permissão Tauri | Se o comando IPC novo `dispatch_action` precisa de permissão explícita em Tauri v2. **PENDENTE:** confirmar na Task 8 e registrar o achado em `sonar-system-design.md` §6 (design §6) |

---

## 7. Plano de implementação

Ordem: F0, F1, F2, F3. Cada fase tem aceite automático e manual no Windows 11 build 26300. Os passos manuais estão na §9.3.

| Fase | Escopo | Aceite automático | Aceite manual | Status |
|---|---|---|---|---|
| **F0** | `actions.rs`: enum `Action` e resolvedor de alvos. Comportamento igual | `cargo test` com 93 passando e 4 ignorados, `bun run test` com 122 passando (linha base em `VERIFICACAO.md`). Testes novos de parsing da `Action` | Trocar saída de app, mutar, volume e perfil pela UI: nada muda | Pode começar após B1 e B2 |
| **F1** | Dispatcher (fila + worker). Bandeja e atalho migrados. A2, A4, A6, A7, A8 | Idempotência de A4. Coalescência de A6. Fila não bloqueia. Fake com `Gone` não derruba o worker | Atalho muta e desmuta. Volume sobe e desce em passo. Atalho ativa perfil. Desconectar headset não derruba o app. Repetir com rede desligada e com app de destino fechado | **PENDENTE S4** para os padrões. Até lá, atalho de teste |
| **F2** | Bindings, validação, registro por binding, migração, A9 | Migração com `.bak`. Duplicata recusada. Troca com falha mantém o anterior | Trocar atalho com painel aberto e fechado. Reiniciar e confirmar que o atalho persiste | **PENDENTE** S4 e desenho da tela de bindings (§6) |
| **F3** | Ciclo de perfil e próxima saída (A3, A10) | Ciclo volta ao primeiro. Perfil apagado não quebra o ciclo. Uma saída só não faz nada | Ciclar com 3 perfis. Apagar o perfil atual e ciclar de novo. Ciclar saídas com uma oculta | **PENDENTE** Q2 (D4) e S4 |

Fora desta fila: F4 (microfone, depende de S1) e F5 (EQ, depende de decisões e spikes S3 e S5).

---

## 8. Critérios de aceite da versão

- [ ] As ações A2, A3, A4, A6, A7, A8, A9 e A10 rodam por atalho e por IPC, com o mesmo resultado.
- [ ] `cargo test` e `bun run test` sem regressão: pelo menos 93 e 122 testes passando, como na linha base.
- [ ] `bun run build` passa, sem avisos de compilador.
- [ ] Checklist da §9.3 completo no Win11 build 26300. Falhas anotadas com passo, resultado e versão do Windows.
- [ ] Migração de `shortcut` testada com um `settings.json` real da v1 (cópia).
- [ ] Resultado do spike S4 registrado no design.
- [ ] Nada da §1 "Não entra" foi implementado.

---

## 9. Testes

### 9.1 Unitários (sem Windows)

Usar os fakes de teste do `Engine` (design §2.7, item L8).

- Idempotência: `set` duas vezes gera uma mudança (RF-06).
- Coalescência e clamp de A6; clamp de A7 (RF-08, RF-09).
- Ciclo de A3 e A10, incluindo perfil apagado e uma saída só (RF-04, RF-05, RF-12).
- Parsing de `Action`: tipo desconhecido rejeitado (RF-01).
- Validação de bindings: duplicata, sintaxe, tamanho (RF-16).
- Migração de `shortcut` com e sem `.bak` (RF-19).
- Binding com alvo apagado devolve erro e é mantido (RF-18).

### 9.2 Integração do dispatcher

- N ações enfileiradas chegam ao `Engine` na ordem de chegada.
- `Gone` em uma ação não derruba o worker (RNF-05).
- Fila cheia não bloqueia o produtor (RNF-02).
- Toggle lê o estado mais recente antes de decidir (RF-07).

### 9.3 Manuais (Win11 build 26300)

Windows 10 não testado: fica fora desta versão, como na v1.

| Atalho ou ação | Passos | Resultado esperado |
|---|---|---|
| `Ctrl+Alt+M` (A4) | Tocar música. Apertar duas vezes | Muta e desmuta. O ícone de volume do Windows acompanha |
| `Ctrl+Alt+↑/↓` (A6, proposta) | Apertar cinco vezes rápido | Volume muda em um ajuste só, sem travar |
| `Ctrl+Alt+1` (A2) | Perfil "Jogo" | Rotas mudam. Apps fora do perfil não mudam |
| `Ctrl+Alt+P` (A3) | Três perfis. Apertar quatro vezes | Ciclo 1, 2, 3, 1 |
| A8 (binding de teste) | Mudo para `discord.exe`, com Discord tocando | Só o Discord muda |
| A9 (binding de teste) | "Spotify para Headset" | Spotify troca de saída |
| A10 (binding de teste) | Duas saídas conectadas | Padrão alterna |
| Dispositivo removido | Desligar o headset e apertar o atalho de mudo | Mensagem de erro. App segue usável |
| Sem rede | Desligar a rede e apertar um atalho | Ação funciona |
| Foco em outro app | Foco no navegador ou no Word, apertar um atalho | Ação funciona |
| App de destino fechado | Fechar o app de A8 ou A9 e apertar o atalho | A8 devolve resultado vazio. A9 guarda a regra |

---

## 10. Riscos

Os riscos do design (§7) valem aqui, em especial a API de roteamento não documentada, que muda entre builds.

| Risco | Mitigação |
|---|---|
| **Conflito de atalho com app ou com o sistema** (S4 pendente) | Erro de registro tratado (RF-17). Padrões só depois do S4 |
| Toggle com leitura de estado desatualizada (teclas rápidas) | Toggle resolvido sob `apply` (RF-07). A API continua com `set` |
| A10 troca o padrão por `IPolicyConfig`, que não é documentada (design §5) | Mesmo isolamento do design. Teste só no build 26300 |
| Rajada de volume gera muitas escritas | Coalescência (RF-08) |
| Migração perde configuração | `.bak` antes de gravar (RF-19) |
| Plugin roda o handler na thread de UI e bloqueia | RF-02. Confirmar o comportamento do plugin (hipótese, design §5) |

---

## 11. Perguntas em aberto para o autor

**Q1. Atalho de mudo (`Ctrl+Alt+M`): toggle ou set?**
Proposta: o atalho faz **toggle** resolvido sob `apply` (lê o estado atual), e a ação na API continua com `set`.
Motivo: o design é contraditório. O §11.1 chama a tecla de toggle, e o §7.2 grava `set_output_mute muted: true` para a mesma tecla. Preciso que você confirme.

**Q2. Persistir o cursor do ciclo de perfil (D4)?**
Proposta: **não persistir**. O ciclo começa no primeiro perfil quando o app abre.

**Q3. Padrões de volume (`Ctrl+Alt+↑/↓`) e de A8, A9 e A10?**
Proposta: deixar sem atalho padrão até o S4 (conflitos). Os padrões de perfil, mudo e ciclo de perfil continuam como estão no design.

**Q1 a Q3 foram fechadas em 2026-11-05** (ver "Decisões fechadas" no plano da v1.1): Q1 — o atalho de mudo é toggle; Q2 — o cursor do ciclo não persiste; Q3 — volume tem padrão `Ctrl+Alt+↑` / `Ctrl+Alt+↓` (±0,05), e A8, A9 e A10 ficam sem atalho padrão. As perguntas abaixo são as que só o autor pode responder agora.

**Q4. O binding `toggle_panel` pode ficar sem atalho?**
Proposta: **sim**. O `toggle_panel` pode ser removido como qualquer outro binding; a linha "Atalho global" do Mockup E passa a mostrar "Nenhum atalho. Toque para definir.". A alternativa (proibir a remoção) foi descartada: quem não quiser atalho global ficaria sem como desligar.
Motivo: o desenho local tomou a decisão D3 e a marcou como pendente de confirmação. `Settings.shortcut` é o espelho do acelerador do `toggle_panel`, então a remoção tem que ser refletida nos dois lugares.

**Q5. "Restaurar padrões" substitui a lista inteira ou só readiciona os que faltam?**
Proposta: **substitui** a lista inteira pelos padrões, com confirmação inline. A alternativa descartada era só readicionar os que faltam.
Motivo: decisão D4 do desenho, marcada como pendente de confirmação. Substituir é reversível por "Sugerir atalhos de perfil" e pelo botão de restore; readicionar deixa bindings obsoletos no lugar.

**Q6. Setas entram na captura de atalho?**
Proposta: **sim** — `ArrowUp`/`ArrowDown` viram `Up`/`Down` na captura, senão o usuário não consegue recapturar `Ctrl+Alt+↑/↓`.
Motivo: hoje o `shortcutFromKeys` só aceita letras, números e F1–F12; setas caem em erro (achado do desenho). Depende do S4 e do que o plugin aceita.

**Q7. O DESIGN da tela de bindings será aprovado?**
Proposta: **aprovar** o rascunho local (`bindings-screen-draft.html`, 62 KB) e publicar no Doop antes da F2.
Motivo: os mockups são vinculantes (design §3) e a publicação no Doop ficou pendente. A Task 16 vai ser implementada a partir do rascunho e entra no PR marcado como "aguardando aprovação visual".

---

## 12. Referências

- [sonar-system-design.md](sonar-system-design.md): design completo (catálogo §4, dispatcher §6, bindings §7.2, fases §10, decisões §11).
- [superpowers/specs/2026-11-02-audio-control-design.md](superpowers/specs/2026-11-02-audio-control-design.md): spec v1, aguardando aprovação. Não sobrescrever.
- [DECISOES.md](DECISOES.md): decisões do autor.
- [VERIFICACAO.md](VERIFICACAO.md): checklist manual da v1.
