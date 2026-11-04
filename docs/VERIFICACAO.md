# Verificação da v1 do Sonar

Checklist do spec §8 (`docs/superpowers/specs/2026-11-02-audio-control-design.md`) mais os
itens de Shell e de desempenho da Task 14. Marque cada item no PC de casa (Windows 11, build
26300) depois de rodar o app com `bun run tauri dev` ou com o instalador gerado.

Antes de começar: abra o Sonar pela bandeja (o ícone fica no canto da barra de tarefas) e tenha
pelo menos um app com som (Spotify, navegador ou similar) e um fone ou caixa de som conectados.

## Roteamento por app

- [ ] **Spotify entre Caixa e Headset.** Abra o Spotify tocando música. No painel, aba Saída,
  toque no chip do Spotify e escolha o Headset. O som sai no Headset. Escolha a Caixa: o som
  volta para a Caixa, sem reiniciar o Spotify.
- [ ] **Discord no microfone.** Entre numa chamada (ou abra o teste de microfone do Discord).
  Na aba Microfone, toque no chip do Discord e escolha o microfone do headset. O medidor do
  microfone padrão mexe quando você fala no microfone escolhido.
- [ ] **Reconectar o headset.** Com o Spotify roteado para o Headset, desligue o Headset (ou tire
  o cabo). O painel mostra "desconectado: usando o padrão do Windows" e o som vai para o padrão.
  Ligue de novo. Em alguns segundos o Spotify volta para o Headset, sem ação sua.
- [ ] **Fechar e reabrir apps.** Feche o Spotify e abra de novo. A regra vale: o Spotify volta a
  sair pelo dispositivo escolhido.
- [ ] **Reiniciar o Windows.** Com regras salvas, reinicie o PC e abra o Sonar. As regras estão
  no painel ("N regras salvas") e os apps saem pelos dispositivos escolhidos quando voltam a tocar.
- [ ] **Perfil "Jogo".** Crie um perfil (barra de perfis, botão "+"): ícone de controle, nome
  "Jogo", Spotify -> Caixa e Discord -> Headset (com o microfone do headset). Salve e toque no
  perfil. As rotas mudam na hora. Toque em "Padrão do Windows" nos apps e confira que voltam ao
  padrão.
- [ ] **Escolha de saída é instantânea.** Com vários apps abertos, troque a saída de um app.
  O chip muda na hora, sem esperar o painel recarregar. Se o Windows recusar, o chip volta ao
  anterior e aparece um aviso.

## Atalho global e configurações

- [ ] **Atalho padrão Win+Alt+V.** Com o painel fechado, pressione Win+Alt+V: o painel abre.
  Pressione de novo: fecha. Com o painel aberto e o foco em outra janela, o atalho também abre.
- [ ] **Trocar o atalho.** Abra Configurações (engrenagem), toque em "Atalho global" e pressione
  Ctrl+Alt+K. O atalho novo funciona e o antigo (Win+Alt+V) não abre mais o painel.
- [ ] **Atalho em uso.** Nas Configurações, tente um atalho que outro programa use (ex.: Ctrl+Shift+Esc
  se estiver livre ou outro conhecido). Deve aparecer o aviso "Não foi possível registrar o
  atalho ... Outro programa pode estar usando essa combinação." e o atalho anterior continua
  valendo.
- [ ] **Iniciar com o Windows (ligar).** Nas Configurações, ligue "Iniciar com o Windows". No
  PowerShell rode `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Sonar`. O valor
  existe e aponta para o caminho do Sonar entre aspas.
- [ ] **Iniciar com o Windows (desligar).** Desligue a opção e rode de novo o `reg query`. O
  valor "Sonar" não existe mais (mensagem "não foi possível encontrar").
- [ ] **Abre na inicialização.** Com a opção ligada, reinicie o PC. O Sonar aparece na bandeja
  sem abrir o painel.

## Posição do painel

- [ ] **Abre perto da bandeja.** Com a barra de tarefas embaixo, clique no ícone do Sonar. O
  painel abre acima da barra, no canto direito, perto do ícone. Não aparece no centro da tela.
- [ ] **Atalho abre no mesmo lugar.** Depois de um clique na bandeja, feche o painel e use
  Win+Alt+V. Ele abre na mesma posição.
- [ ] **Atalho sem clique anterior.** Reinicie o Sonar e use Win+Alt+V antes de clicar na bandeja.
  O painel abre no canto inferior direito da tela principal, dentro da área de trabalho.
- [ ] **Barra de tarefas em outro lado (opcional).** Mova a barra para cima ou para a lateral
  (Configurações do Windows > Barra de tarefas > Posição) e clique na bandeja. O painel abre do
  lado oposto da barra e inteiro dentro da tela.
- [ ] **Escala de tela.** Com escala de 125% ou 150% (Configurações > Sistema > Tela), o painel
  continua do tamanho certo e dentro da tela.
- [ ] **Perder o foco esconde.** Com o painel aberto, clique numa janela qualquer: o painel se
  esconde. Clicar no ícone logo em seguida não o reabre (250 ms de intervalo).

## Windows 10

- [ ] **Windows 10 (não testado).** No Windows 10 o roteamento usa a variante "Downlevel" da
  interface de política de áudio (escolha feita por `variant_for_build`), sem teste real. Se houver uma máquina Windows 10, repita os
  itens de roteamento acima e registre o resultado aqui. Sem máquina, a v1 não é garantida no
  Windows 10.

## Após a verificação

- [ ] Itens que falharam foram anotados com o passo, o que aconteceu e a versão do Windows.

## Resultados automáticos (última rodada, 3/11/2026)

Rodado no PC de casa, sem executar o app (sem `tauri dev` e sem `tauri build`, por falta de memória):

- `cd src-tauri && cargo test` (MSVC, Windows 11 SDK instalado): **93 passam, 4 ignorados**
  (os 4 ignorados tocam no áudio real e só rodam com `-- --ignored`). Zero avisos do compilador.
- `bun run test` (Vitest): **122 passam** em 25 arquivos. Uma rodada anterior caiu por timeout
  de carga (24 workers em pouca memória); rodada seguinte passou sem mudança de código.
- `bun run build` (`tsc` + Vite): **passa**.
- `cargo check --release` (perfil de release, sem gerar instalador): **passa**, zero avisos (8 min 49 s com dependências).
- Não rodado: `bun run tauri build` (instalador), `bun run tauri dev` e os itens manuais
  deste arquivo. Eles dependem de você.
