# Zerun

Controla tus agentes de programación (Claude Code, Codex, Cursor, Devin, Grok, Hermes, Pi, Antigravity y DeepSeek Harness) de forma local por defecto, con sincronización opcional entre varios dispositivos.

*[English](README.md) | [简体中文](README.zh-CN.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | Español*

![App de escritorio de Zerun](docs/media/readme/app-screenshot.jpg)

## App de escritorio

Descarga la última versión para tu plataforma desde [GitHub Releases](https://github.com/AndPuQing/zeron/releases/latest):

- **macOS** — `zerun-<version>-macos-arm64.dmg`
- **Windows** — `zerun-<version>-windows-x86_64-setup.exe`
- **Linux** — `zerun-<version>-linux-<arch>.tar.gz`; descomprímelo y ejecuta el `install.sh` que incluye

No necesitas cuenta ni conexión a internet; las sesiones se quedan en tu dispositivo. La app se actualiza sola.

Las instalaciones de escritorio usan el nombre **Zerun** y el ejecutable `zerun`.
Pueden coexistir con Zeron: los datos se guardan en `~/.zerun` en macOS/Linux y
en `%LOCALAPPDATA%\Zerun` en Windows. `ZERUN_DATA_DIR` permite cambiar esta
ubicación. El puerto IPC local predeterminado es 27655 (`ZERUN_IPC_PORT`) y el
puerto de retorno del inicio de sesión es 27642 (`ZERUN_CALLBACK_PORT`).

## Android

Descarga `zerun-<version>-android.apk` desde [GitHub Releases](https://github.com/AndPuQing/zeron/releases/latest).
El APK firmado de publicación admite Android 10 o posterior en dispositivos
arm64. Puedes abrir la demo sin cuenta o iniciar sesión para seguir tus sesiones
sincronizadas. En los ajustes, **Check for updates** descarga y verifica la
actualización y abre el instalador de Android. Consulta
[la configuración y las actualizaciones de Android](apps/android/README.md).

## Modo headless (CLI)

Para servidores y otras máquinas sin pantalla, como un VPS que mantiene los agentes en ejecución después de cerrar tu portátil. Solo Linux:

```bash
curl -fsSL https://edge.550w.host/install.sh | sh
zerun status
```

El instalador arranca el motor como un servicio en segundo plano que sobrevive a los reinicios.

```bash
zerun status      # modo local/sincronizado y estado del motor
zerun update      # actualizar a la última versión
zerun daemon start|stop|restart|status
```

## Sincronización entre dispositivos (opcional)

Inicia sesión para lanzar un agente en un dispositivo y seguirlo o controlarlo desde otro:

```bash
zerun daemon stop
zerun login        # o: zerun logout para volver al modo solo local
zerun daemon start
```

Los dispositivos con sesión iniciada en la misma cuenta pueden leer y escribir los archivos del espacio de trabajo de los demás, así que inicia sesión solo en dispositivos de confianza. Las sesiones locales existentes nunca se suben.

## Patrocinadores

Gracias a [The Context Company](https://www.thecontextcompany.com/) por patrocinar Zeron. Tú también puedes ayudar a financiar el desarrollo de Zeron [convirtiéndote en patrocinador en GitHub](https://github.com/sponsors/zeronsh).

---

¿Quieres contribuir o te interesa saber cómo funciona? [Pregúntale a DeepWiki](https://deepwiki.com/zeronsh/zeron) o consulta [ARCHITECTURE.md](ARCHITECTURE.md).

Publicado bajo la [licencia MIT](LICENSE).
