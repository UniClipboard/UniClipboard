<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="UniClipboard" width="112" height="112" />

# Copie uma vez. Cole em qualquer dispositivo.

O <strong>UniClipboard</strong> sincroniza a área de transferência entre seus computadores.<br />
Sem conta. Sem nuvem. Quase sem uso de memória.

<a href="#quick-start"><img alt="Começar" src="https://img.shields.io/badge/Come%C3%A7ar-→-000000?style=for-the-badge" /></a>
<a href="#features"><img alt="Recursos" src="https://img.shields.io/badge/Recursos-555555?style=for-the-badge" /></a>

<table>
  <tr>
    <td align="center" width="170"><h2>0</h2>contas para criar</td>
    <td align="center" width="170"><h2>0</h2>servidores na nuvem guardando seu conteúdo</td>
    <td align="center" width="170"><h2>14 MB</h2>de memória para o serviço de sincronização<sup><a href="#benchmarks">1</a></sup></td>
    <td align="center" width="170"><h2>6 dígitos</h2>para parear um dispositivo e aprovar</td>
  </tr>
</table>

Windows · macOS · Linux · CLI　|　Texto · Imagens · Arquivos · Pastas

<a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <strong>Português (Brasil)</strong> | <a href="README.ru.md">Русский</a>

</div>

---

## Por que o UniClipboard

- **Multiplataforma**: sincroniza entre Windows, macOS e Linux, na mesma rede Wi-Fi ou em redes diferentes.
- **Sem nuvem**: os dispositivos se conectam diretamente por canais criptografados. Sem cadastro. O histórico fica só nos seus dispositivos.
- **Leve**: o serviço de sincronização usa cerca de 14 MB de memória em repouso<sup>[1](#benchmarks)</sup>. No Windows, a janela principal usa cerca de 34–49 MB<sup>[2](#benchmarks)</sup>. Ao fechá-la, o processo dela termina.
- **Sincroniza tudo**: texto, imagens, arquivos e pastas inteiras.

<a id="features"></a>

## Recursos

| | |
|---|---|
| ⚡ **Painel rápido** | Pressione `⌘ ⌃ V` (`Ctrl Alt V` no Windows / Linux) em qualquer app, escolha um item e pressione Enter para colar. |
| 🔍 **Encontre tudo** | Digite uma palavra ou filtre com `#etiqueta`, `@dispositivo` ou `/tipo`. Encontre o que copiou há meses. |
| 🏷️ **Etiquetas automáticas** | Links, e-mails, cores, números de versão, UUIDs e endereços IP recebem etiquetas automaticamente. Crie suas próprias regras. |
| 📁 **Arquivos e pastas** | Envie uma pasta inteira com a estrutura preservada, para todos os dispositivos ou só um. |
| 📮 **Envio sem pareamento** | `unic send "texto"` mostra um link de transferência e, quando disponível, um código de 6 dígitos. Quem recebe pode usar um ou outro. Nenhum registro de dispositivo fica salvo. |
| 🧩 **Plugins** | Instale plugins com um clique nas Configurações, por exemplo para transformar uma imagem ou arquivo em link de compartilhamento. |
| ⌨️ **Linha de comando** | Use `unic` para enviar, receber e filtrar a lista do histórico pelo terminal, via SSH ou em scripts. |
| 🌍 **Idiomas** | English, 简体中文, 日本語, Português, Русский. Temas claro e escuro. |

<a id="quick-start"></a>

## Comece em 3 passos

**1. Execute**

Baixe a versão de prévia do Desktop v2 para a sua plataforma no [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases). Para executar a partir do código-fonte, use os comandos abaixo.

```sh
git clone https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard
make run-desktop
```

**2. Conecte um segundo dispositivo**

No primeiro dispositivo, abra **Dispositivos → Adicionar dispositivo** e anote o código de 6 dígitos. Digite-o no segundo dispositivo. Quando os dois mostrarem a mesma impressão digital, clique em **Aceitar** em cada um.

**3. Copie**

Pressione `Ctrl+C` em um dispositivo e `Ctrl+V` no outro.

## Linha de comando

```sh
unic send --to all "Hello"        # enviar para todos os dispositivos
unic send --to all -f ./photos    # enviar arquivos ou uma pasta
unic send "temporário"            # sem pareamento: mostra um link e, quando disponível, um código
unic recv 482913                  # receber com o código
unic history list                 # ver o histórico
```

Veja todos os comandos no [guia da CLI](app/service/README.md).

<a id="benchmarks"></a>

## Medições

| | Memória | Condições |
|---|---:|---|
| Serviço `unicd` | **14 MB** | macOS (Apple Silicon), histórico vazio, lido após 20 s em repouso; mesma medida da coluna Memória do Monitor de Atividade<sup>1</sup> |
| Janela principal | **34 MB** / 49 MB | Windows 11, build nativo ARM64 / x64, 60 itens de texto; medida do Gerenciador de Tarefas<sup>2</sup> |
| Janela principal com imagens | 119 MB / 133 MB | Igual, mais 12 imagens de 2048×1536<sup>2</sup> |

<sup>1</sup> Medido no commit `e2cb68d` com `footprint <pid>`. <sup>2</sup> Mediana de três execuções. Veja o [relatório de memória no Windows](docs/exec/windows-desktop-memory.md).

## Privacidade

- O conteúdo é criptografado de ponta a ponta entre dispositivos. O UniClipboard não mantém servidores que guardem sua área de transferência.
- Você confere uma impressão digital e aprova cada novo dispositivo. Um dispositivo removido perde o acesso na hora.
- Conteúdo copiado, caminhos de arquivos e endereços de dispositivos nunca vão para os logs.
- O diagnóstico anônimo não inclui conteúdo nem identidade do dispositivo. Desative quando quiser em **Configurações → Diagnóstico e privacidade**.

Relate problemas de segurança de forma privada, conforme o [SECURITY.md](SECURITY.md).

## Atualizando do UniClipboard v1

O v2 importa todo o seu histórico do v1: texto, imagens, arquivos e etiquetas. Siga o assistente ao abrir o v2 pela primeira vez ou importe depois em **Configurações › Armazenamento › Migrar da v1**.

Dispositivos v2 não sincronizam com dispositivos v1. Atualize todos para o v2 e pareie novamente. O código do v1 está no [branch `v1`](https://github.com/UniClipboard/UniClipboard/tree/v1).

## Contribuindo

```sh
make build            # dist/
go test -race ./...
```

Leia primeiro o [CONTRIBUTING.md](CONTRIBUTING.md) e a [documentação de arquitetura](docs/architecture/README.md). Para criar um plugin, veja o [guia de plugins](plugins/README.md).

## Licença

[AGPL-3.0](LICENSE)
