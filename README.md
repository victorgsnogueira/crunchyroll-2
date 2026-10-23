# Crunchyroll Desktop

Client nativo da Crunchyroll para Windows, em Rust + Slint. Uso pessoal: usa a
marca da Crunchyroll e uma API não oficial, então **não distribua**.

## Rodar

```
cargo run --release -p crunchyroll-desktop
```

O executável fica em `target/release/crunchyroll.exe`.

## Como funciona

- `crates/core`: acesso à API (via [crunchyroll-rs](https://github.com/crunchy-labs/crunchyroll-rs)), dados das telas, cache de imagens.
- `crates/player`: o vídeo. Um WebView2 sem bordas, dentro da janela, com Shaka Player e controles no visual da Crunchyroll. O DRM é feito pelo módulo licenciado do próprio Edge; o app só repassa os bytes da licença.
- `app`: interface Slint (`app/ui`) e o controlador (`app/src`).

## Fontes

A interface usa Lato. Os títulos usam a Crunchyroll Atyp se ela estiver em
`assets/fonts/private/CrunchyrollAtyp-Bold.ttf` (essa pasta fica fora do git).

## Arquivos do app

`%APPDATA%\local\CrunchyrollDesktop\data`:
- `app.log`: log geral.
- `playback-check.log`: qual DRM foi usado e a maior resolução decodificada (teste de 1080p).

A sessão (refresh token) fica no Gerenciador de Credenciais do Windows.

## Variáveis de ambiente (desenvolvimento)

- `CR_ANON=1`: entra como visitante, sem conta (catálogo/busca; sem player).
- `CR_DEV_PAGE=browse|search|simulcast|...` e `CR_DEV_DETAIL=<id da série>`: abre direto numa tela.
- `CR_DRM_ORDER=widevine|playready|playready,widevine`: ordem de DRM no player.
- `CR_WEBVIEW_ARGS`: argumentos extras para o WebView2.
- `CR_PLAYER_DEBUG=1`: mostra resolução/DRM no player (também com a tecla D).
