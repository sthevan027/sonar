# Decisões do projeto (sessão de brainstorming, 2026-10-31/11-01)

Origem: o mixer de volume do Windows 10/11 exige muitos cliques e não permite deixar, por exemplo, a música na caixa e Discord/Chrome no headset de forma simples.

## Decidido pelo Sthevan

- **Objetivo:** ferramenta pessoal **e** produto. Gratuito. Open source ou código fechado: **ainda não decidiu** (não bloqueia a v1).
- **Stack:** Tauri (Rust + React/TypeScript), mesma do Focus Brew. É um app **separado** do Focus Brew; só reaproveita o padrão de bandeja/popup/atalho global.
- **Design:** próprio, não seguir a linha do EarTrumpet ("estressante"). Nunca usou apps parecidos.
- **Escopo da v1:** nível 1 (volume/mute por app, trocar saída padrão) + nível 2 (roteamento por app). **Fora:** equalizador/efeitos (exigem driver de áudio virtual).
- **Abrir o painel:** clique no ícone da bandeja (+ atalho global como base; rodinha sobre o ícone como bônus, palpite do Claude).
- **Regras lembradas:** o app lembra a rota de cada app e reaplica sozinho. Exceção: se o dispositivo some (ex.: headset Redragon desconectado), o app cai pro padrão do Windows; **ao reconectar, volta sozinho**.
- **Perfis:** opcionais, criados pelo usuário, editáveis; salvam um conjunto de apps → saídas (ex.: "Jogo"). O perfil só mexe nos apps que lista; os demais ficam como estavam.
- **Ativação do perfil:** sempre manual (nada de detecção automática de jogo).
- **Identificação do perfil:** por **ícone** (ex.: controle para jogo), alterável pelo usuário. Palpite do Claude: nome opcional também (acessibilidade/desambiguar).
- **Persistência:** arquivo JSON (padrão do Focus Brew, `directories::ProjectDirs`). SQLite se crescer. Redis descartado (é servidor).
- **Windows suportado:** 11 e 10 na v1. O PC de casa só valida o 11 (build 26300); o 10 fica "implementado, não testado" até haver teste real.

## Fatos técnicos verificados

- Roteamento por app funciona no Win11 26300 via `AudioPolicyConfig` (ver README). Sem driver virtual.
- Rota está ligada ao **PID do processo em execução** → regras automáticas precisam reaplicar quando o app volta a emitir som (ouvir `IAudioSessionNotification`).
- Chrome usa processo de áudio separado: o PID certo pode não ser o da janela principal (**não verificado**).
- Não existe crate Rust pronto; interface declarada na mão com o crate `windows` (0.62.x) usando o C# do EarTrumpet como referência.

## Ainda em aberto

- O que o painel mostra e como se roteia na UI (dropdown por app? arrastar?). Pedir mockup.
- Rota **sobrevive** a fechar/reabrir o app? E a reiniciar o Windows? (não testado)
- Comportamento exato quando o dispositivo some (um relato diz que o app fica mudo até trocar a saída; o Sthevan quer cair no padrão).
- Posicionar o popup perto do ícone da bandeja (o Focus Brew não faz isso).
- Nome do produto. Licença / código aberto ou fechado.
- Windows 10: testar a variante Downlevel.

## Próximos passos sugeridos

1. Terminar rodada de perguntas (painel, dispositivo que some, nome).
2. Testar persistência da rota (fechar/reabrir app, reiniciar).
3. Escrever o design formal (`docs/superpowers/specs/…-design.md`) e só então o plano de implementação.
