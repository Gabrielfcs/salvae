<p align="center">
  <img src="crates/salvae-ui/assets/bot-logo.png" width="200" alt="Salvaê">
</p>

<h1 align="center">Salvaê</h1>

<p align="center">
  <b>Sincronize os saves dos seus jogos co-op com o grupo — automático, seguro e sem servidor.</b>
</p>

<p align="center">
  <img alt="Rust" src="https://img.shields.io/badge/feito%20com-Rust-orange">
  <img alt="Plataforma" src="https://img.shields.io/badge/Windows-10%2F11-blue">
  <img alt="Licença" src="https://img.shields.io/badge/licença-MIT-green">
  <img alt="Open source" src="https://img.shields.io/badge/open--source-sim-success">
</p>

---

## O problema

Em jogos co-op (Valheim, Terraria, Palworld, Lethal Company…) o save mora na
máquina de **quem hospeda** a sessão. Se essa pessoa não está online — ou
esquece de mandar o arquivo — ninguém continua a campanha.

## A solução

O **Salvaê** mantém o save mais recente de cada jogo disponível para qualquer
membro do grupo poder hospedar. Ele **puxa o save mais novo quando você abre o
jogo** e **envia o seu quando você fecha** — sem você precisar pensar nisso.

## Por que o Salvaê

- 🔌 **Sem servidor próprio.** Nada de infra pra manter: os saves trafegam por um
  canal privado do **Discord do seu próprio grupo**.
- 🔒 **Seguro por padrão.** Os saves são **cifrados no seu PC** (Argon2id +
  AES-256-GCM) com a senha do grupo. O Discord só vê bytes embaralhados.
- 🪶 **Leve.** Escrito em Rust, binário único, fica quietinho na bandeja até um
  jogo abrir.
- 🔍 **Código aberto.** Tudo auditável — você sabe exatamente o que roda na sua
  máquina.
- 🎮 **Você no controle.** Só sincroniza os jogos que **você** configurar.

## Como funciona

```
        abre o jogo                          fecha o jogo
            │                                     │
            ▼                                     ▼
   pull: baixa o save mais          push: envia seu save como
   recente do canal e aplica        nova versão (com histórico)
            │                                     │
            └──────────►  canal privado do Discord  ◄──────────┘
                         (cofre cifrado do grupo)
```

- **Versionamento:** cada jogo guarda um save canônico + as últimas N versões.
- **Proteção contra conflito:** se existir um save mais novo que o seu, o app
  avisa em vez de sobrescrever — você decide (manter o remoto ou enviar o seu).
- **Aviso de simultâneo:** mostra se outro membro está jogando o mesmo jogo
  naquele momento.

## Instalação

1. Baixe o `salvae-ui.exe` (ou compile — veja abaixo).
2. Rode. Na primeira vez, uma tela de boas-vindas explica o app.
3. Crie um grupo seguindo o **assistente** (ele guia a criação do bot do
   Discord, passo a passo).

> Requer Windows 10/11.

## Criando um grupo (resumo)

O assistente do app cuida disso, mas em resumo:

1. **Dono:** cria um bot no [Portal de Desenvolvedores do Discord](https://discord.com/developers/applications),
   copia o **token**, e usa o app pra adicionar o bot ao servidor e escolher o
   canal (`#saves`). Define **nome do grupo + senha**.
2. **Amigos:** colam o **convite** gerado + a **senha** (combinada por fora).
   Pronto — todos compartilham o mesmo cofre.

> A senha nunca é guardada: o app deriva uma chave dela e protege essa chave
> com a **DPAPI** do Windows.

## Compilando

Pré-requisitos: [Rust](https://rustup.rs) (toolchain MSVC no Windows).

```bash
# app de desktop (bandeja + janela)
cargo build --release -p salvae-ui
# binário em target/release/salvae-ui.exe

# testes
cargo test --workspace
```

## Arquitetura

Workspace Cargo dividido por responsabilidade:

| Crate            | Responsabilidade                                                    |
|------------------|---------------------------------------------------------------------|
| `salvae-core`    | Cripto (Argon2id, AES-256-GCM), compressão, hash, versão            |
| `salvae-vault`   | Abstração do cofre (`Channel`) + registros das versões              |
| `salvae-discord` | Transporte real via API REST do Discord (bot token)                 |
| `salvae-config`  | Grupos, convites cifrados, `config.toml`, segredos via DPAPI        |
| `salvae-sync`    | Motor de sync: pull / push / resolução de conflito                  |
| `salvae-detect`  | Catálogo de jogos (Steam/Epic) + descoberta de pasta de save        |
| `salvae-watch`   | Observa abrir/fechar de processos e detecta os jogos                |
| `salvae-agent`   | Liga observação → detecção → sync por grupo                         |
| `salvae-ui`      | Janela (egui) e o worker de sync em segundo plano                   |

## Segurança

- Saves cifrados **client-side**; a confidencialidade depende da **senha do
  grupo**, não do Discord nem do token do bot.
- O token do bot é uma credencial **por grupo**, escopada só ao canal do grupo.
- Segredos locais (token + chave derivada) protegidos pela **DPAPI** do Windows.

## Roadmap

- **Rodar em segundo plano na bandeja do sistema.** Hoje, fechar a janela (X)
  encerra o app; para continuar sincronizando, basta deixá-lo minimizado (o
  worker de sync roda independente da janela). O objetivo é fechar para a
  bandeja, com "Abrir/Sair" funcionando de verdade. Isso exige assumir o event
  loop do `winit` (em vez do `eframe::run_native`), porque com a janela
  escondida o `eframe` para de chamar `update` e os cliques da bandeja não são
  processados.

## Licença

MIT.
