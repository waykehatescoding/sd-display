# Deck Display

Turns a 15 key Stream Deck (MK.2, Original 2019, Scissor Keys, 15 Key Module) into a small status display on Windows.

Widgets:

- Clock and date
- Current weather and 5 day forecast (OpenWeatherMap)
- CPU, GPU, RAM, disk and network usage
- Page switching and buttons that open apps, files or websites

Widgets can span multiple keys. The image is laid out with the 25 px of screen that sits behind the bezel between keys, so graphs and backgrounds line up across keys instead of looking squished.

It runs in the background with no window and uses around 7 MB of memory. Opening it from the Start menu brings up its settings page in your browser.

## Install

Download `DeckDisplaySetup.exe` from the [latest release](../../releases/latest) and run it.

Quit the Elgato Stream Deck app first. Only one program can drive the keys at a time.

For weather, get a free API key from https://home.openweathermap.org/api_keys and paste it into Settings along with your city.

## Build

```
cargo build --release
```

The installer is built with [Inno Setup](https://jrsoftware.org/isinfo.php) from `installer.iss`. Pushing a `v*` tag builds it and attaches it to a release.

## Notes

- Settings are saved to `%APPDATA%\streamdeckdisplay\config.json`.
- The settings page only listens on 127.0.0.1.
- Stats are only read while a widget on the current page needs them.
- GPU load comes from Windows performance counters, so it works with AMD, Intel and NVIDIA.
