# Crunchyroll Desktop — documento de design

Data: 2026-10-23
Status: aprovado pelo dono em 2026-10-23

## 1. Intenção e o porquê

Um client de Crunchyroll para PC (Windows), escrito em Rust, para uso pessoal do dono do projeto com a própria conta paga.

O problema não é a funcionalidade do app oficial — o dono acha o app bom. O problema é que **ele tem "ar de site"**: parece uma página web dentro de uma janela. O objetivo é o app que a Crunchyroll faria **se ela focasse em fazer algo excelente para PC**: fiel à identidade visual dela, bonito, e rápido.

Prioridades declaradas pelo dono, nesta ordem de importância igual:
1. **Beleza**
2. **Identidade visual da Crunchyroll** (cores, logo, fontes, estilo)
3. **Performance**

## 2. Objetivos

- Todas as funcionalidades possíveis do app oficial, acessadas pela API da Crunchyroll (engenharia reversa da API usada pelos apps/site oficiais).
- Interface 100% nativa (sem página web nenhuma), exceto a superfície de vídeo do player (ver §5).
- Assistir episódios **sem sair do app** e **sem precisar do app oficial**.
- Sensação de app nativo: resposta imediata, animações suaves, nada de telas em branco ou spinners por toda parte, atalhos de teclado.

## 3. Não-objetivos

- Download / assistir offline (esbarra no DRM).
- Linux / macOS nesta versão (o player depende de componente do Windows).
- Distribuição pública. O app usa marca, logo e fontes da Crunchyroll e uma API não oficial: é **estritamente pessoal**, nunca publicado.
- Gerenciamento de assinatura / pagamento (feito no site oficial).

## 4. Anti-objetivos (fracasso mesmo que "funcione")

- Qualquer coisa que pareça site: barras de rolagem de navegador, flashes brancos, layout "de página", elementos HTML visíveis.
- Quebrar, contornar ou extrair o DRM. **Proibido.** O vídeo só é descriptografado por um CDM licenciado rodando como foi projetado (ver §5).
- App lento: travadas na rolagem, imagens pipocando sem transição, abertura demorada.
- Identidade genérica — tem que ser reconhecível como Crunchyroll à primeira vista.

## 5. Restrição central: DRM e o player

Os streams da Crunchyroll são protegidos por **Widevine/PlayReady**. Um app Rust não pode obter licença de CDM do Google/Microsoft por conta própria, e extrair/contornar o CDM é proibido (§4).

**Decisão:** toda a UI é nativa; **somente o retângulo do vídeo** é uma superfície **WebView2** (motor do Edge, já presente no Windows 11), sem bordas, filha da janela nativa, carregando uma página local mínima com **Shaka Player** (open source) que:
- recebe do Rust o manifesto (DASH) e os dados de licença (URL do proxy de licença + token/headers) obtidos pela API;
- reproduz via EME usando o CDM licenciado do Edge, exatamente como o site oficial faz;
- não exibe nenhum controle web — **os controles do player são desenhados pela UI nativa**, por cima/ao redor, e comandam o vídeo via mensagens (WebView2 `PostWebMessage` ↔ `window.chrome.webview`).

Detalhe de composição a resolver no build: controles nativos sobre um HWND filho exigem uma janela overlay transparente (top-level sem borda, sincronizada) ou desenhar os controles dentro da própria página do player com o mesmo visual. Preferência: **overlay nativo**; fallback aceitável: controles na página, desde que visualmente idênticos ao design nativo. Fica a cargo do builder, validado no spike.

**Plano B** (se o WebView2 não obtiver licença da Crunchyroll): trocar apenas o motor do player por **CEF** (Chromium Embedded) com Widevine via component updater. O resto do app não muda — a superfície do player deve ficar atrás de uma interface/trait própria para que a troca seja local.

**Primeira tarefa do projeto: spike de viabilidade** (descartável): login → obter um episódio → tocar no WebView2 com Shaka. Se falhar com Widevine, testar PlayReady; se falhar ambos, testar CEF.

**Critério de sucesso (decidido pelo dono): o spike só passa se reproduzir em 1080p.** Tocar limitado a 720p (ou menos) conta como falha daquele motor/DRM, e o spike avança para a próxima opção (WebView2+Widevine → WebView2+PlayReady → CEF+Widevine). O resultado decide o motor antes de construir o resto. Se nenhum funcionar sem violar §4, o projeto para e volta para conversa com o dono.

## 6. Experiência e telas

### 6.1 Navegação (decidido pelo dono: opção A)
**Barra lateral fixa à esquerda**, estilo programa de PC (Spotify/Discord):
- Topo: logo Crunchyroll.
- Ícones: Início, Busca, Explorar, Simulcasts, Minha lista, Histórico.
- Base: avatar do perfil (troca de perfil, configurações, sair).
- *Chamada minha:* ao passar o mouse, a barra expande com os nomes (animação).
- Barra de título própria (sem a moldura padrão do Windows), com minimizar/maximizar/fechar no estilo do app.

### 6.2 Telas
- **Login**: e-mail/senha digitados pelo usuário no app; token guardado com segurança (Windows Credential Manager); refresh automático.
- **Seleção de perfil** (multi-perfil da Crunchyroll).
- **Início**: destaque grande com arte (hero) e botão "Continuar TX EY" / "+ Lista"; fileiras da própria Crunchyroll (Continuar assistindo, recomendações, novidades, etc. — seguir o feed da home da API).
- **Busca**: resultados enquanto digita, com debounce.
- **Explorar**: catálogo, filtros por gênero, ordenação (populares, novos, A–Z), dublado/legendado.
- **Simulcasts**: temporada atual + calendário semanal de lançamentos.
- **Minha lista**: watchlist + Crunchylists (criar, renomear, apagar, adicionar/remover).
- **Histórico**: episódios assistidos, com opção de remover.
- **Página do anime**: arte de fundo, sinopse, classificação, gêneros, seletor de temporada, lista de episódios com progresso, idiomas de áudio/legenda disponíveis, adicionar à lista, avaliar.
- **Player** (ver 6.3).
- **Configurações**: idioma de áudio e legenda preferidos, qualidade padrão, autoplay, conta.
- Notificações/novos episódios da watchlist: se a API expuser, mostrar badge/indicador; *chamada minha*, prioridade baixa.

### 6.3 Player (aprovado pelo dono via mockup)
Tela cheia dentro da janela; barra lateral some; controles somem quando o mouse para.
- Topo: voltar, título do anime, temporada/episódio e nome.
- Barra de progresso laranja com buffer; **faixas marcando abertura, encerramento e recap**; miniatura ao passar o mouse se a API fornecer.
- Controles: play/pause, −10s/+10s, próximo episódio, volume, tempo, legendas, configurações, tela cheia.
- Botão **"Pular abertura"** (e encerramento/recap) quando o trecho estiver ativo.
- Menu único com **Áudio** (troca dublado/legendado = troca de versão), **Legendas**, **Qualidade**, **Velocidade**.
- Fim do episódio: cartão **"A seguir"** com contagem regressiva e autoplay.
- Retoma de onde parou e **envia progresso** para a conta (como o app oficial).
- Atalhos: Espaço, ←/→ (10s), ↑/↓ volume, F tela cheia, M mudo, S pular trecho, N próximo, Esc sair da tela cheia.
- Legendas: renderização respeitando estilo (ASS/VTT). Se o Shaka não renderizar ASS fielmente, usar libass-wasm/SubtitlesOctopus na página do player. Fica a cargo do builder.

## 7. Identidade visual

- **Cores**: laranja Crunchyroll `#F47521` como cor de destaque; fundos pretos/cinza-escuro do produto (`#000`, `#141519`, `#23252B`); texto branco/cinza (`#FFFFFF`, `#A0A0A0`). O builder deve conferir os valores exatos contra o app/site atual.
- **Logo**: o oficial (símbolo + wordmark), em vetor.
- **Fonte**: a fonte da marca é a **Crunchyroll Atyp** (rebrand de 2024), proprietária e sem download público. Decisão: o app carrega os arquivos da fonte se o dono colocá-los numa pasta do app; senão, usa uma fonte livre próxima (o builder escolhe e documenta). A UI do site/app pode usar fonte diferente da marca — conferir.
- **Ícones**: estilo dos ícones oficiais (traço, cantos), em SVG.
- Estilo geral: escuro, arte dos animes como protagonista, laranja usado com moderação, botões retangulares em caixa alta como no produto atual.

## 8. Movimento e animação (decidido)

- **Animações de interface em código** (Slint): transições de tela, hover em cards (escala/brilho/infos), barra lateral expandindo, fade dos controles do player, imagens com transição de borrado → nítido, skeletons com shimmer no lugar de spinners.
- Um **conjunto único de curvas e durações** (um "manual de movimento") usado em todo o app para consistência.
- **Lottie** só onde fizer sentido: abertura do app (logo), estados vazios, talvez loading. Slint não toca Lottie nativamente: o builder escolhe como (ex.: renderizar quadros com crate de Lottie em Rust para imagens, ou pré-renderizar em sequência de quadros). Blender não é usado.

## 9. Performance

- Abrir até a tela inicial utilizável rapidamente; mostrar dados em cache imediatamente e atualizar em segundo plano.
- Cache local em disco de imagens (com versões redimensionadas) e de respostas da API.
- Rolagem fluida em listas longas (virtualização).
- Rede e decodificação de imagens fora da thread de UI (async).
- Metas concretas ficam a cargo do builder; a régua é "nunca parecer mais lento que o app oficial".

## 10. Como é construído (decisões já tomadas)

- **Linguagem**: Rust (toolchain instalada: 1.99).
- **UI**: **Slint** (renderização na GPU, animações declarativas). *Chamada minha.*
- **Plataforma**: Windows 11.
- **Player**: WebView2 + Shaka Player numa página local; plano B CEF (§5).
- **API**: cliente HTTP async próprio para a API da Crunchyroll (auth/token, perfis, home feed, busca, catálogo, séries/temporadas/episódios, watchlist, Crunchylists, histórico, progresso, streams/licença). Referências públicas: projetos como crunpyroll (Python) e o plugin Kodi CrunchyREroll. A API é não oficial e pode mudar: isolar num módulo/crate próprio com tipos claros.
- **Credenciais**: nunca em texto puro em disco; usar Windows Credential Manager.

## 11. O que fica para o builder

- Estrutura de crates/módulos, escolha das bibliotecas (HTTP, async, cache, WebView2 binding, Lottie).
- Mecanismo exato de overlay dos controles sobre o vídeo (§5).
- Paleta exata, espaçamentos, tamanhos de tipografia, conjunto de ícones, conferidos contra o produto atual.
- Ordem de implementação após o spike (sugestão: API+login → início → página do anime → player → resto).
- Estratégia de testes (API com respostas gravadas; UI testada manualmente).

## 12. Decisões complementares (chamadas minhas, após revisão)

- **Identidade de cliente oficial**: o dono pediu engenharia reversa do app oficial. O app pode se apresentar à API como um client oficial (client ID, user-agent, headers e, na página do player, a origem esperada pelo servidor de licença). Isso é permitido. Continua proibido qualquer coisa que toque na criptografia: extrair chaves, usar CDM extraído, gravar o vídeo descriptografado.
- **Login**: principal = e-mail/senha em tela nativa. Fallback se bloqueado = fluxo de ativação por código (estilo TV: "digite este código em crunchyroll.com/activate"), também nativo. Não usar janela web para login.
- **Design das outras telas**: seguem a estrutura funcional do app oficial, redesenhadas no visual nativo deste documento. **Início** e **Página do anime** passam por mockup aprovado pelo dono antes de serem construídas; as demais seguem o mesmo sistema visual sem checkpoint.
- **Escopo de "todas as funcionalidades"**: tudo de vídeo que a API expuser, incluindo, além de §6.2: edição de perfil/avatar, controles de maturidade/parentais, avaliações e (se existir na API) comentários. **Fora**: mangá, Game Vault, loja e notícias (são outros produtos).
- **Idioma**: interface em pt-BR (strings centralizadas para permitir tradução futura); conteúdo, catálogo e região seguem a conta/perfil.
- **Assets**: o builder obtém logo e ícones em vetor a partir do site/app oficial (uso pessoal) e cria as animações Lottie no estilo da marca. Fonte: ver §7.

## 13. Ainda em aberto

- (Resultado do spike decide WebView2 vs PlayReady vs CEF.)
