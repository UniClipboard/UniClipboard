# unic Android

尚未开始。计划:原生 Kotlin / Gradle 工程,通过 `bindings/` 嵌入公开的 Go `engine`(见 [0003](../../docs/design/decisions/0003-engine-sdk.md))。

先验证的范围:前台文本的发送与接收应用,再扩展图片、文件和系统允许的后台场景。Android 对后台读剪贴板有限制,宿主通过 `HostCapabilities` 如实声明,不要用额外权限绕过。
