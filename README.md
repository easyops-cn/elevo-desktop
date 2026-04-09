# Elevo Messenger Desktop

Elevo Messenger is a matrix client focusing primarily on simple, elegant and secure interface. The desktop app is made with Tauri.

## Download

Installers for macOS, Windows and Linux can be downloaded from [Github releases](https://github.com/easyops-cn/elevo-desktop/releases).

Operating System | Download
---|---
Windows | <a href='https://github.com/easyops-cn/elevo-desktop/releases/latest/download/Elevo.Messenger_1.0.5_x64_en-US.msi'>Get it for Windows</a>
macOS Apple Silicon | <a href='https://github.com/easyops-cn/elevo-desktop/releases/latest/download/Elevo.Messenger_1.0.5_aarch64.dmg'>Get it for macOS Apple Silicon</a>
macOS Intel | <a href='https://github.com/easyops-cn/elevo-desktop/releases/latest/download/Elevo.Messenger_1.0.5_x86_64.dmg'>Get it for macOS Intel</a>
Linux | <a href='https://github.com/easyops-cn/elevo-desktop/releases/latest/download/Elevo.Messenger_1.0.5_amd64.AppImage'>Get it for Linux</a>

## Local development

Firstly, to setup Rust, NodeJS and build tools follow [Tauri documentation](https://v2.tauri.app/start/prerequisites/).

Now, to setup development locally run the following commands:
* `git clone --recursive https://github.com/easyops-cn/elevo-desktop.git`
* `cd elevo-desktop/cinny`
* `npm ci`
* `cd ..`
* `npm ci`

To build the app locally, run:
* `npm run tauri build`

To start local dev server, run:
* `npm run tauri dev`

## Publishing

### App Store

* `npm run tauri build -- --no-bundle --target universal-apple-darwin --config src-tauri/tauri.appstore.conf.json`
* `npm run tauri bundle -- --bundles app --target universal-apple-darwin --config src-tauri/tauri.appstore.conf.json --skip-stapling`
* `xcrun productbuild --sign "<Mac Installer Distribution certificate signing identity>" --component "src-tauri/target/universal-apple-darwin/release/bundle/macos/Elevo Messenger.app" /Applications "Elevo Messenger.pkg"`
* `xcrun altool --upload-app --type macos --file "Elevo Messenger.pkg" --apiKey $APPLE_API_KEY --apiIssuer $APPLE_API_ISSUER`

## License
This project is forked from [cinnyapp/cinny-desktop](https://github.com/cinnyapp/cinny-desktop), which is licensed under AGPL-3.0.

This project continues to use the same AGPL-3.0 license.
