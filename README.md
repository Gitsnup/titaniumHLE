# titaniumHLE: high-level emulator for iPhone OS apps

**titaniumHLE** is a fork of [touchHLE](https://touchhle.org/), a high-level emulator for iPhone OS apps. It runs on modern desktop operating systems and Android, and is written in Rust.

titaniumHLE's high-level emulation (HLE) approach differs from low-level emulation (LLE) in that it does not directly simulate the iPhone/iPod touch hardware. Instead of running iPhone OS inside emulation, titaniumHLE _itself_ takes the place of iPhone OS and provides its own implementations of the system frameworks (Foundation, UIKit, OpenGL ES, OpenAL, etc). The only code the [emulated CPU](https://github.com/merryhime/dynarmic) executes is the app binary and [a handful of libraries](touchHLE_dylibs/).

The goal of this fork is the same as upstream — to run games from the early days of iOS:

* Currently: iPhone, iPod touch and iPad apps for iPhone OS 2.x, iPhone OS 3.x, and iOS 4.0.x.
* Longer term: high-DPI (“Retina Display”) support, newer iOS 4 versions, iOS 5.x, iOS 6.x.
* 64-bit iOS (ARM64): possible, but very experimental. Upstream has ruled it out [as a goal](https://github.com/touchHLE/touchHLE/issues/181#issuecomment-1777098259), and this fork keeps the dynarmic A32 pipeline for real apps; a separate, minimal ARM64 interpreter (`src/arm64.rs`) exists for simple 64-bit binaries and is under active development. Full 64-bit HLE (UIKit, OpenGL ES, etc. for ARM64) is not implemented.

**This does not mean that all apps for these OS versions work.** The vast majority of iPhone OS 2.x and iPhone OS 3.x apps do not currently work in titaniumHLE, and the ones that do work are generally games (support for other apps isn't a priority: it's more complex and less fun). This improves gradually over time. The upstream [touchHLE app compatibility database](https://appdb.touchhle.org/) is still a useful reference for which apps are known to work.

## What this fork changes

titaniumHLE is an experimental fork used for rapidly prototyping features that may or may not be upstreamed. Compared with upstream touchHLE, it currently includes:

* An **app picker with a Frutiger Aero-inspired design**, more closely matching the glossy look of early iPhone OS.
* An in-app **"Add game" flow on Android**: a document picker copies `.ipa` files into the app directory, with no manual file management needed (the Files-app exploit used by older flows has long been patched, so this replaces it).
* **Split CI workflows** per platform, including **Android debug and release builds**, with release APKs signed using a stable keystore so builds are upgradable across releases.
* Various small fixes and additions made while getting specific early apps running (e.g. local multiplayer-style networking options carried over and extended, pause-gesture support, debug tooling for view hierarchies).

This is a personal/hobby fork; for the polished, canonical version of the project, see upstream touchHLE at <https://touchhle.org/>.

## Important disclaimer

This project is not affiliated with or endorsed by Apple Inc in any way. iPhone, iOS, iPod, iPod touch and iPad are trademarks of Apple Inc in the United States and other countries.

Only use titaniumHLE to emulate software you have obtained legally.

## Platform support

* Officially supported in this fork: x64 Windows, x64 macOS and AArch64 Android (CI builds all three).
  * If you're an Apple Silicon Mac user, the x64 build reportedly works in Rosetta.
* Probably works, but you must build it yourself: AArch64 macOS, x64 Linux, AArch64 Linux.
* Never?: other architectures.

Input methods:

- For simulated touch input, there are four options:
  - Mouse/trackpad input (tap/hold/drag by pressing the left mouse button)
  - Virtual cursor using a game controller (move the cursor with the right analog stick, and tap/hold/drag by pressing the stick or the right shoulder button)
  - Mapping of game controller buttons or the left analog stick to specific on-screen locations (see the descriptions of `--button-to-touch=`, `--dpad-to-touch=` and `--stick-to-touch=` in `OPTIONS_HELP.txt`)
  - Real touch input, if you're on a device that has a touch screen
- For simulated accelerometer input, there are three options:
  - Tilt control simulation using the left analog stick of a game controller
  - Tilt control simulation using a mouse (hold down the right mouse button)
  - Real accelerometer input, if you are using a phone, tablet or some other device with a built-in accelerometer (TODO: support game controllers with accelerometers)

## Development status

titaniumHLE inherits upstream touchHLE's development history (in development since December 2022, originally [hikari\_no\_yume](https://hikari.noyu.me/)'s full-time passion project, with many volunteer contributors since). This fork adds experimental work on top of it, and there are no promises about its future either. Please be patient.

In general, the supported functionality is defined by the supported apps: most work is driven by getting a particular game running, and contributing support for whichever missing features it needs. Consequently, the completeness varies a lot between APIs, e.g. UIKit is easily the most hacky and incomplete of the large frameworks that have been implemented, because most games don't use very much of its functionality, whereas the OpenGL ES and OpenAL implementations are probably complete enough to cover a large number of early apps, because games make heavy use of these.

# Usage

First obtain titaniumHLE, either a [binary release](https://github.com/Gitsnup/titaniumHLE/releases) or by building it yourself (see the next section).

You'll then need an app that you can run. The upstream [app compatibility database](https://appdb.touchhle.org/) is a good guide for which versions of which apps are known to work, but bear in mind that it may contain outdated or inaccurate information. Note that the app binary must be decrypted to be usable.

There's a few ways you can run an app in titaniumHLE.

## Special Android notes

Windows, Mac and Linux users can skip this section.

On Android, only the graphical user interface (app picker) is available. Therefore, you must put your “.ipa” files or “.app” bundles inside the “touchHLE\_apps” directory. Note that you can only do that once you have run titaniumHLE at least once.

File management can be tricky on Android due to [restrictions introduced by Google in newer Android versions](https://developer.android.com/about/versions/11/privacy/storage#scoped-storage). One of these methods may work:

* Tap the **“Add game”** button in titaniumHLE. This opens your device's document picker; choose a “.ipa” file and it will be copied into titaniumHLE's apps directory automatically. titaniumHLE closes itself while the picker is open, so just open it again afterwards to see the new game in the app picker. If you have multiple “.ipa” files to add, simply repeat this for each one.
* If you have an older version of Android, you may be able to directly access titaniumHLE's files by browsing to `/sdcard/Android/data/org.touchhle.android/files/touchHLE_apps`. Note that the `/sdcard` directory is usually not on the SD card.
* You may be able to use ADB. If you're unfamiliar with ADB, try using <https://yume-chan.github.io/ya-webadb/> (in Google Chrome or another browser with WebUSB) with your device connected over USB. titaniumHLE's files can be found in “sdcard” > “Android” > “data” > “org.touchhle.android” > “files” > “touchHLE\_apps”.

## Graphical user interface

titaniumHLE has a built-in app picker, restyled with a Frutiger Aero-inspired look. If you put your `.ipa` files and `.app` bundles in the `touchHLE_apps` directory, they will show up in the app picker when you run titaniumHLE.

To configure the options, you can edit the `touchHLE_options.txt` file. To get a list of options, look in the `OPTIONS_HELP.txt` file.

## Command-line user interface

**This section does not apply on Android.**

You can see the command-line usage by passing the `--help` flag.

If you're a Windows user and unfamiliar with the command line, these instructions may help you get started:

1. Move the `.ipa` file or `.app` bundle to the same folder as `titaniumHLE.exe`.
2. Hold the Shift key and right-click on the empty space in the folder window.
3. Click “Open with PowerShell”.
4. Type `.\titaniumHLE.exe "YourAppNameHere.ipa"` (or `.app` as appropriate) and press Enter. If you want to specify options, add a space after the app name (outside the quotes) and then type the options, separated by spaces.

## Local multiplayer support

titaniumHLE provides limited support for local multiplayer via Wi-Fi in some games. At the moment of writing it is supported in Asphalt 4 and N.O.V.A.

Real iOS devices could also join/host games!

**Usage:**
1. Install titaniumHLE on 2+ devices connected to the same Wi-Fi network.
2. **Important:** Ensure titaniumHLE is whitelisted in your OS firewall/network settings.
3. Enable "Network access" in Quick options or via `--allow-network-access`.
4. Start/join multiplayer in the game.

**FAQs:**
* **Tunneling over Internet/VPN:** Not officially supported, but might work.
* **Bluetooth:** Not supported.

**Known issues:**
* On macOS you may need to launch titaniumHLE from terminal as otherwise OS will block network connections.

## Other stuff

Any data saved by the app (e.g. **saved games**) are stored in the `touchHLE_sandbox` folder.

If the emulator crashes almost immediately while running a **known-working** version of a game, please check whether you have any overlays turned on like the Steam overlay, Discord overlay, RivaTuner Statistics Server, etc. Sadly, as useful as these tools are, they work by injecting themselves into other apps or games and don't always clean up after themselves, so they can break titaniumHLE… it's not our fault. 😢 Currently only RivaTuner Statistics Server is known to be a problem. If you find another overlay that doesn't work, please tell us about it.

# Building and contributing

See the `CONTRIBUTING.md` file in the git repo if you want to contribute. If you just want to build titaniumHLE, look at `dev-docs/building.md`.

# License

titaniumHLE is a fork of touchHLE; both projects' contributions are covered as follows.

touchHLE © 2023–2026 touchHLE project contributors.

The source code of touchHLE and titaniumHLE themselves (not their dependencies) are licensed under the Mozilla Public License, version 2.0.

Due to license compatibility concerns, binaries are under the GNU General Public License version 3 or later.

For a best effort listing of all licenses of dependencies, build titaniumHLE and pass the `--copyright` flag when running it, or click the “Copyright info” button in the app picker.

Please note that different licensing terms apply to the bundled dynamic libraries (in `touchHLE_dylibs/`) and fonts (in `touchHLE_fonts/`). Please consult the respective directories for more information.

# Thanks

We stand on the shoulders of giants. Thank you to:

* Everyone who has contributed to touchHLE or titaniumHLE, or supported any of its contributors financially.
* [hikari\_no\_yume](https://hikari.noyu.me/) and all upstream touchHLE contributors, for creating the project this fork builds on.
* The authors of and contributors to the many libraries used by this project: [dynarmic](https://github.com/merryhime/dynarmic), [rust-macho](https://github.com/flier/rust-macho), [SDL](https://libsdl.org/), [rust-sdl2](https://github.com/Rust-SDL2/rust-sdl2), [stb\_image](https://github.com/nothings/stb), Imagination Technologies' [PVRTC decompressor](https://github.com/powervr-graphics/Native_SDK/blob/master/framework/PVRCore/texture/PVRTDecompress.cpp), [openal-soft](https://github.com/kcat/openal-soft), [hound](https://github.com/ruuda/hound), [Symphonia](https://github.com/pdeljanov/Symphonia), [RustType](https://gitlab.redox-os.org/redox-os/rusttype), [the Liberation fonts](https://github.com/liberationfonts/liberation-fonts), [the Noto CJK fonts](https://github.com/googlefonts/noto-cjk), [rust-plist](https://github.com/ebarnard/rust-plist), [nibarchive](https://github.com/michaelwright235/nibarchive), [quick-xml](https://github.com/tafia/quick-xml), [gl-rs](https://github.com/brendanzab/gl-rs), [cargo-license](https://github.com/onur/cargo-license), [cc-rs](https://github.com/rust-lang/cc-rs), [cmake-rs](https://github.com/rust-lang/cmake-rs), [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), [cargo-ndk-android-gradle](https://github.com/willir/cargo-ndk-android-gradle), [md-5 and sha1](https://github.com/RustCrypto/hashes), [encoding_rs](https://github.com/hsivonen/encoding_rs), [corosensei](https://github.com/Amanieu/corosensei), [uuid](https://github.com/uuid-rs/uuid) and the Rust standard library.
* The Skyline emulator project (RIP), for [writing the tedious boilerplate needed to replace file management on newer Android versions](https://github.com/skyline-emu/skyline/blob/dc20a615275f66bee20a4fd851ef0231daca4f14/app/src/main/java/emu/skyline/provider/DocumentsProvider.kt).
* The [Rust project](https://www.rust-lang.org/) generally.
* The various people out there who've documented the iPhone OS platform, officially or otherwise. Much of this documentation is linked to within this codebase!
* The iOS hacking/jailbreaking community.
* The Free Software Foundation, for making libgcc and libstdc++ copyleft and therefore saving this project from ABI hell.
* The National Security Agency of the United States of America, for [Ghidra](https://ghidra-sre.org/).
* [GerritForge](http://www.gerritforge.com/) for providing free Gerrit hosting to the general public, including us.
* The many contributors to [Gerrit](https://www.gerritcodereview.com/).
* Many friends who took an interest in the project and gave suggestions and encouragement.
* Developers of early iPhone OS apps. What treasures you created!
* Apple, and NeXT before them, for creating such fantastic platforms.