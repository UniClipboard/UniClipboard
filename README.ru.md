<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# Больше пользы от каждого копирования.

<p>
  <strong>Unic (ранее UniClipboard)</strong> — нативный менеджер буфера обмена с приоритетом локального хранения и синхронизацией между устройствами для Windows, macOS, Linux, iOS, Android и терминала.<br />
  Быстрый поиск по истории, быстрая панель, умный предпросмотр, плагины и сквозная P2P-синхронизация — без аккаунта и без облака.
</p>

<p>
  <a href="https://uniclipboard.app"><img alt="Официальный сайт" src="https://img.shields.io/badge/Website-0891B2?style=for-the-badge" /></a>
  <a href="https://docs.uniclipboard.app"><img alt="Документация" src="https://img.shields.io/badge/Docs-4F46E5?style=for-the-badge&logo=readthedocs&logoColor=white" /></a>
  <a href="#community"><img alt="Группа WeChat" src="https://img.shields.io/badge/WeChat-07C160?style=for-the-badge&logo=wechat&logoColor=white" /></a>
  <a href="#community"><img alt="Группа QQ" src="https://img.shields.io/badge/QQ-1688D8?style=for-the-badge&logo=qq&logoColor=white" /></a>
  <a href="https://t.me/uniclipboard"><img alt="Группа Telegram" src="https://img.shields.io/badge/Telegram-26A5E4?style=for-the-badge&logo=telegram&logoColor=white" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/stargazers"><img alt="Звёзды на GitHub" src="https://img.shields.io/github/stars/UniClipboard/UniClipboard?style=flat-square&color=E5B650&logo=github&label=Stars" /></a>
  <a href="LICENSE"><img alt="Лицензия AGPL-3.0" src="https://img.shields.io/badge/License-AGPL--3.0-22c55e?style=flat-square" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Windows" src="https://img.shields.io/badge/Windows-1f2937?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0iIzM4YmRmOCI+PHBhdGggZD0iTTAgMGg3LjR2Ny40SDB6bTguNiAwSDE2djcuNEg4LjZ6TTAgOC42aDcuNFYxNkgwem04LjYgMEgxNlYxNkg4LjZ6Ii8+PC9zdmc+" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="macOS" src="https://img.shields.io/badge/macOS-1f2937?style=flat-square&logo=apple&logoColor=white" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Linux" src="https://img.shields.io/badge/Linux-1f2937?style=flat-square&logo=linux&logoColor=facc15" /></a>
  <a href="https://testflight.apple.com/join/nyNQ8dQe"><img alt="iOS Beta" src="https://img.shields.io/badge/iOS-Beta-2563eb?style=flat-square&logo=apple&logoColor=white&labelColor=1f2937" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Android" src="https://img.shields.io/badge/Android-1f2937?style=flat-square&logo=android&logoColor=3ddc84" /></a>
  <a href="#cli"><img alt="CLI" src="https://img.shields.io/badge/CLI-1f2937?style=flat-square&logo=gnubash&logoColor=white" /></a>
</p>

<p>
  <small>
    <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <strong>Русский</strong>
  </small>
</p>

![Unic на компьютере, планшете и смартфоне](docs/assets/product-overview.png)

</div>

---

## Почему Unic

- **Больше, чем синхронизация — полноценный менеджер буфера обмена**: большинство менеджеров привязаны к одной ОС или платному облаку, а инструменты синхронизации не дают удобной истории и быстрой вставки. Unic полезен даже на одном компьютере — мгновенная быстрая панель, локальный поиск, автоматические метки и карточки предпросмотра ссылок — и бесшовно синхронизирует текст, изображения, файлы и целые папки между Windows, macOS, Linux, iOS, Android и терминалами в одной сети Wi-Fi или через разные сети.
- **Нативная скорость, а не тяжеловесный Electron**: написан на Go с нативным десктопным интерфейсом. Фоновая служба (`unicd`) занимает около 14 МБ памяти в простое; в Windows главное окно занимает около 34–49 МБ и **полностью завершает UI-процесс при закрытии**, не расходуя память в фоне.
- **Приватность на уровне архитектуры, а не на словах**: в буфере обмена регулярно оказываются пароли, API-ключи и личные файлы. Unic не требует аккаунта и не хранит данные в облаке — устройства соединяются напрямую по сквозному шифрованному каналу после сверки отпечатков, удаление устройства мгновенно отзывает доступ, а история остаётся только на ваших устройствах.
- **Программируемый рабочий процесс, а не просто копирование**: автоматическая классификация, умный предпросмотр ссылок с поиском по заголовку и описанию страницы, безопасные **действия (Actions)** без изменения исходной записи (очистка форматирования, преобразование текста, создание ссылки на файл), **Go- и Lua-плагины** в один клик из встроенного каталога и полноценный **CLI (`unic`)** для SSH, скриптов и разовой передачи по 6-значному коду без сопряжения.

<a id="features"></a>

## Возможности

<table>
  <tr>
    <td>⚡ <strong>Быстрая панель</strong></td>
    <td>Нажмите <code>⌘ ⌃ V</code> (<code>Ctrl Alt V</code> в Windows / Linux) в любом приложении, найдите запись и нажмите <code>Enter</code> для вставки или <code>Shift+Enter</code> для вставки без форматирования.</td>
  </tr>
  <tr>
    <td>🔍 <strong>Мгновенный локальный поиск</strong></td>
    <td>Ищите по ключевым словам или фильтруйте по <code>#метке</code>, <code>@устройству</code>, <code>/типу</code> и избранному. Находите записи многомесячной давности — включая ссылки по заголовку и описанию страницы.</td>
  </tr>
  <tr>
    <td>🏷️ <strong>Автоматические метки и предпросмотр</strong></td>
    <td>Ссылки, e-mail, цвета, номера версий, UUID и IP-адреса получают метки автоматически (можно добавить свои regex-правила), а для ссылок формируются карточки предпросмотра.</td>
  </tr>
  <tr>
    <td>🔄 <strong>Синхронизация устройств и папок</strong></td>
    <td>Синхронизируйте текст, форматированный текст, изображения, файлы и целые папки с сохранением структуры — на все сопряжённые устройства или на одно.</td>
  </tr>
  <tr>
    <td>📮 <strong>Отправка без сопряжения</strong></td>
    <td><code>unic send "текст"</code> выводит ссылку для передачи и, если доступен, 6-значный код. Получатель может использовать любой из них, запись об устройстве не сохраняется.</td>
  </tr>
  <tr>
    <td>🧩 <strong>Действия и плагины</strong></td>
    <td>Запускайте действия без изменения исходной записи (преобразование текста, очистка стилей, превращение изображения или файла в ссылку) и устанавливайте Go- или Lua-плагины в один клик из настроек.</td>
  </tr>
  <tr>
    <td>⌨️ <strong>Командная строка</strong></td>
    <td><code>unic</code> позволяет отправлять и принимать данные, запускать действия и фильтровать историю из терминала, по SSH или в скриптах.</td>
  </tr>
  <tr>
    <td>🌍 <strong>Языки и темы</strong></td>
    <td>English, 简体中文, 日本語, Português, Русский. Светлая и тёмная темы.</td>
  </tr>
</table>

<a id="quick-start"></a>

## Начните за 3 шага

**1. Скачайте и установите**

- **Десктоп**: скачайте установщик для Windows (`.exe`/`.zip`), macOS (`.dmg`) или Linux (`.deb`/`.rpm`/`.AppImage`) на странице [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases).
- **iOS**: присоединитесь к публичному бета-тестированию через [TestFlight](https://testflight.apple.com/join/nyNQ8dQe).
- **Android**: скачайте и установите APK со страницы [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) (публикация в Google Play пока не планируется).
- **Только CLI**: выполните `npm install -g @uniclipboard/cli`.

[![Packaging status](https://repology.org/badge/vertical-allrepos/uniclipboard.svg)](https://repology.org/project/uniclipboard/versions)

**2. Пользуйтесь сразу**

Копируйте как обычно и нажимайте `⌘ ⌃ V` (`Ctrl Alt V` в Windows / Linux) в любом приложении, чтобы открыть быструю панель, найти запись и вставить её.

**3. Подключите второе устройство (необязательно)**

На первом устройстве откройте **Устройства → Добавить устройство** и запомните 6-значный код. Введите его на втором устройстве. Когда оба покажут одинаковый отпечаток, нажмите **Принять** на каждом — после этого всё скопированное будет синхронизироваться автоматически.

<a id="cli"></a>

## Командная строка

```sh
unic send --to all "Hello"        # отправить на все устройства
unic send --to all -f ./photos    # отправить файлы или папку
unic send "временно"              # без сопряжения: ссылка и код, если он доступен
unic recv 482913                  # принять по коду
unic history list                 # история
```

Все команды — в [руководстве по CLI](app/service/README.md).

## Конфиденциальность

- Данные шифруются сквозным шифрованием между устройствами. У Unic нет серверов, которые хранят ваш буфер обмена.
- Каждое новое устройство вы одобряете сами, сверив отпечаток. Удалённое устройство сразу теряет доступ.
- Содержимое буфера, пути к файлам и адреса устройств не попадают в журналы.
- Анонимная диагностика не содержит данных буфера и идентификаторов устройств. Её можно отключить в **Настройки → Диагностика и конфиденциальность**.

О проблемах безопасности сообщайте конфиденциально, как описано в [SECURITY.md](SECURITY.md).

## Переход с UniClipboard v1

v2 импортирует всю историю v1: текст, изображения, файлы и метки. Следуйте подсказкам при первом запуске v2 или импортируйте позже через **Настройки › Хранилище › Перенести из v1**.

Устройства v2 не синхронизируются с устройствами v1. Обновите все устройства до v2 и выполните сопряжение заново. Код v1 находится в [ветке `v1`](https://github.com/UniClipboard/UniClipboard/tree/v1).

<a id="community"></a>

## Сообщество

Присоединяйтесь к сообществу Unic, чтобы общаться с пользователями и разработчиками, делиться идеями, задавать вопросы и сообщать о проблемах:

- <img src="docs/assets/community/qq.svg" alt="" width="18" /> Группа QQ: `1080860954`
- <img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat: отсканируйте QR-код ниже, чтобы добавить мейнтейнера и присоединиться к группе
- <img src="docs/assets/community/telegram.svg" alt="" width="18" /> [Группа Telegram (@uniclipboard)](https://t.me/uniclipboard)

<table>
  <tr>
    <td align="center"><strong><img src="docs/assets/community/qq.svg" alt="" width="18" /> Группа QQ</strong></td>
    <td align="center"><strong><img src="docs/assets/community/wechat.svg" alt="" width="18" /> Группа WeChat</strong></td>
    <td align="center"><strong><img src="docs/assets/community/telegram.svg" alt="" width="18" /> Telegram</strong></td>
  </tr>
  <tr>
    <td align="center"><img src="docs/assets/community/qq-qr.png" alt="QR-код группы QQ Unic" width="240" /></td>
    <td align="center"><img src="docs/assets/community/wechat-qr.png" alt="QR-код WeChat Unic" width="240" /></td>
    <td align="center"><a href="https://t.me/uniclipboard"><img src="docs/assets/community/telegram-qr.jpg" alt="QR-код группы Telegram Unic" width="240" /></a></td>
  </tr>
</table>

## Участие в разработке

```sh
make build            # dist/
go test -race ./...
```

Сначала прочитайте [CONTRIBUTING.md](CONTRIBUTING.md) и [документацию по архитектуре](docs/architecture/README.md). Как написать плагин — в [руководстве по плагинам](plugins/README.md).

## Лицензия

[AGPL-3.0](LICENSE)
