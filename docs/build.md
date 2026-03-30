# Build Guide

This guide matches the current repository layout.

## 1. Build the Rust workspace

```powershell
cd D:\New project\unofficial-tg-rs
cargo build --workspace
```

Useful entry points:

```powershell
cargo run -p tg-cli -- manifest
cargo run -p tg-cli -- tdlib-probe --tdjson vendor/tdlib/tdjson.dll
cargo run -p tg-cli -- bridge-download --tdjson vendor/tdlib/tdjson.dll --file-id 123 --chat-id 0 --size 8388608
cargo run -p tg-cli -- bridge-upload --tdjson vendor/tdlib/tdjson.dll --path .\sample.bin --chat-id 123456789
cargo run -p tg-desktop
```

## 2. Build TDLib for this repo

Official TDLib sources:

- <https://github.com/tdlib/td>
- <https://core.telegram.org/tdlib/docs/>
- <https://tdlib.github.io/td/build.html?language=Rust>

The official TDLib README says the generic build flow is:

```text
mkdir build
cd build
cmake -DCMAKE_BUILD_TYPE=Release ..
cmake --build .
```

It also says that for low-memory builds you can target `tdjson` directly.

### Windows example

The commands below are an inference from the official generic CMake flow, adapted for a normal Visual Studio x64 build on Windows:

```powershell
git clone https://github.com/tdlib/td.git
cd td
mkdir build
cd build
cmake -A x64 -DCMAKE_BUILD_TYPE=Release ..
cmake --build . --config Release --target tdjson
```

After the build succeeds, copy the produced `tdjson.dll` into this repository:

```powershell
New-Item -ItemType Directory -Force D:\New project\unofficial-tg-rs\vendor\tdlib | Out-Null
Copy-Item .\Release\tdjson.dll D:\New project\unofficial-tg-rs\vendor\tdlib\tdjson.dll
```

The desktop app and CLI both look for `tdjson` in these locations:

- `vendor/tdlib/tdjson.dll`
- `bin/tdjson.dll`
- `tdjson.dll`

## 3. Run the desktop shell with TDLib probing

```powershell
cd D:\New project\unofficial-tg-rs
cargo run -p tg-desktop
```

What you can do in the app:

- probe `tdjson` to verify the dynamic library loads
- issue a real `getAuthorizationState` request through TDLib
- preview `setTdlibParameters`
- preview `downloadFile`, `addFileToDownloads`, `preliminaryUploadFile`, and `sendMessage` document requests
- bridge acceleration plans into a real logged-in TDLib session
- generate acceleration plans for upload and download work

## 4. Notes

- The current desktop app is a development shell, not a finished Telegram client.
- Real login still requires your own `api_id` and `api_hash` from <https://my.telegram.org>.
- The TDLib JSON interface is the intended bridge for this Rust rewrite because TDLib officially recommends the JSON interface for languages that can call C functions.
- The new `bridge-download` and `bridge-upload` commands assume the TDLib session is already logged in and that the configured TDLib database points to that existing session.
