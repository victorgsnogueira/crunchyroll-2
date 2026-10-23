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

## Login

A Crunchyroll não aceita mais login por senha direto na API (o app oficial
de celular também não usa). Por isso há dois jeitos de entrar:

- **E-mail e senha**: a página oficial `sso.crunchyroll.com` aparece num painel
  dentro do app (como no app de celular). Ao terminar, o app captura o
  redirecionamento `sso.crunchyroll://auth?code=...` e troca o código pela sessão
  (OAuth com PKCE, cliente do app Android de celular).
- **Código como na TV**: o app mostra um código para digitar em
  crunchyroll.com/activate (cliente do app Android TV).

As credenciais de cliente dos apps oficiais são buscadas do repositório
`crunchy-labs/artifacts` e guardadas por um dia.

## Fontes

A interface usa Lato. Os títulos usam a Crunchyroll Atyp se ela estiver em
`assets/fonts/private/CrunchyrollAtyp-Bold.ttf` (essa pasta fica fora do git).

## Arquivos do app

`%APPDATA%\local\CrunchyrollDesktop\data`:
- `app.log`: log geral.
- `playback-check.log`: qual DRM foi usado e a maior resolução decodificada (teste de 1080p).

A sessão (refresh token) fica no Gerenciador de Credenciais do Windows.

## Variáveis de ambiente (desenvolvimento)

- `CR_DEV_SSO=1`: abre direto o painel de login oficial (sem usar a sessão salva).
- `CR_ANON=1`: entra como visitante, sem conta (catálogo/busca; sem player).
- `CR_DEV_PAGE=browse|search|simulcast|...` e `CR_DEV_DETAIL=<id da série>`: abre direto numa tela.
- `CR_DRM_ORDER=widevine|playready|playready,widevine`: ordem de DRM no player.
- `CR_WEBVIEW_ARGS`: argumentos extras para o WebView2.
- `CR_PLAYER_DEBUG=1`: mostra resolução/DRM no player (também com a tecla D).
