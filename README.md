<div align="center">

<img src="assets/icons/industrial/Keycaps-Titanium-K.iconset/icon_128x128@2x.png" width="128" height="128" alt="">

# Keycaps

<img src="assets/readme/hero.svg" width="100%" alt="Quiet keys. Mechanical sound. Thirteen recorded switches and seven mice, played from your Mac's menu bar.">

<br><br>

<a href="https://github.com/Walid-Idrissi-Labs/KeyCaps/releases/latest/download/Keycaps.dmg"><img src="assets/readme/download.svg" height="48" alt="Download for macOS"></a>&nbsp;&nbsp;&nbsp;<a href="#build-from-source"><img src="assets/readme/source.svg" height="48" alt="Build from source"></a>

<sub>Free and open source · macOS 11 or later · Apple silicon and Intel</sub>

</div>

<br>

Keycaps is a menu bar app that plays the recording of a real mechanical switch for every key you press, in any app. It comes with thirteen switches and seven mice, places each key between the speakers where it sits on the keyboard, and uses no CPU at all while you are not typing.

<br>

<div align="center">

<img src="assets/readme/stats.svg" width="100%" alt="13 recorded switches. 7 mice, button by button. 0% CPU while you rest. 0 key presses stored or sent. 2 ms from sample start to the hit. 3% pitch drift from stroke to stroke. 2.9 MB download, sounds included. 0 dependencies.">

<br><br><br>

<sub>ONE MENU</sub>

## Everything is in the menu

There is no window and no Dock icon. One keycap in the menu bar holds every setting.

</div>

<table>
<tr>
<td width="56%">
<img src="assets/readme/menu.svg" width="100%" alt="The Keycaps menu: today's key count, Enable Keycaps, Keyboard Volume, Mouse Volume, Switch Keys, Mouse Clicks, Spatial Audio, Key-up sounds, Scroll ticks and Quit. The Switch Keys list is open and Holy Panda is being previewed.">
</td>
<td valign="top">
<br>
<b>1 &nbsp; One shortcut turns it off</b>
<br>
<kbd>⇧</kbd> <kbd>⌘</kbd> <kbd>K</kbd> switches Keycaps off and on from any app. Off means off: it stops listening to the keyboard altogether.
<br><br><br>
<b>2 &nbsp; Hear a switch before you choose it</b>
<br>
Point at a switch or a mouse in the list and it plays. Click to keep it.
<br><br><br>
<b>3 &nbsp; A count of today's keys</b>
<br>
The first line shows how many keys you have pressed today. That number is the only thing Keycaps remembers about your typing.
</td>
</tr>
</table>

<br><br>

<div align="center">

<sub>THE SOUND</sub>

## What makes it sound real

<img src="assets/readme/spatial.svg" width="760" alt="A chart of the home row from A to the apostrophe key: A is loud in the left speaker and quiet in the right, H is equal in both, and the apostrophe is the reverse.">

</div>

<table>
<tr>
<td width="50%" valign="top">
<b>Every key in its place</b>
<br>
A comes from the left, L from the right and the space bar from the middle. Spatial Audio sets how far apart they are: Off, Subtle, Natural or Wide.
</td>
<td width="50%" valign="top">
<b>No two strokes alike</b>
<br>
Each stroke plays up to 3% above or below its recorded pitch, so fast typing never sounds like one sample on repeat.
</td>
</tr>
<tr>
<td valign="top">
<b>Recorded row by row</b>
<br>
A full switch pack has a recording for each of the five rows and for space, return and delete, and four more for keys coming back up.
</td>
<td valign="top">
<b>Mice too</b>
<br>
The left button, right button and wheel click of seven mice, each with its press and its release, at a volume of their own.
</td>
</tr>
<tr>
<td valign="top">
<b>As loud as you set it</b>
<br>
Keycaps follows the Mac's volume and compensates for it, so the clicks stay where you put them while you turn your music up or down, until the Mac gets too quiet for that. Mute still mutes.
</td>
<td valign="top">
<b>The whole keyboard</b>
<br>
Brightness, volume and media keys click as well. Holding a key down gives one click, not a burst, and the lock button stays silent.
</td>
</tr>
</table>

<br><br>

<div align="center">

<sub>PRIVACY</sub>

## It listens for keys. It keeps none of them.

An app that is told about every key press has to earn that. This is everything Keycaps does with one.

</div>

<table>
<tr>
<td width="50%" valign="top">
<b>It can only listen</b>
<br>
The keyboard hook is created listen-only. Keycaps cannot change, block or type a key.
</td>
<td width="50%" valign="top">
<b>Nothing is stored</b>
<br>
It reads which key moved, to choose the row and the side, plays the sound and forgets it. The one thing kept is a count of today's key presses.
</td>
</tr>
<tr>
<td valign="top">
<b>Nothing is sent</b>
<br>
There is no network code in Keycaps: no analytics, no update check, no account.
</td>
<td valign="top">
<b>No sound in password fields</b>
<br>
While a password field has focus, macOS stops delivering key presses to listeners like Keycaps, so it goes quiet.
</td>
</tr>
</table>

You do not have to take this on trust. The whole app is about 1,600 lines of Rust in [`src`](src), with no third-party code.

<br><br>

<div align="center">

<sub>UNDER THE HOOD</sub>

## Built to cost nothing

</div>

Keycaps is written in Rust directly against the system frameworks (AppKit, Core Graphics, Core Audio), without dependencies. Nothing polls and no timer runs while you are idle. A key press wakes the app, a sound is mixed, and a minute after the last one the audio device is released again.

<table>
<tr><td><b>CPU at rest</b></td><td>0%, with no wake-ups</td></tr>
<tr><td><b>CPU while typing</b></td><td>0.2 to 0.4%</td></tr>
<tr><td><b>Memory</b></td><td>14 MB at rest, about 24 MB while sound is playing</td></tr>
<tr><td><b>Size</b></td><td>0.8 MB of code for both architectures, 2.9 MB to download with every recording</td></tr>
<tr><td><b>Latency</b></td><td>Each recording starts 2 ms before its hit. After a minute of silence the speakers take 10 to 40 ms to wake</td></tr>
<tr><td><b>Dependencies</b></td><td>None</td></tr>
</table>

<sub>Measured on an Apple silicon Mac running macOS 26.</sub>

<br><br>

<div align="center">

<sub>GETTING STARTED</sub>

## Up and typing in three steps

</div>

<table>
<tr>
<td width="33%" valign="top">
<b>1 &nbsp; Download and open</b>
<br>
Open <code>Keycaps.dmg</code>, drag Keycaps to Applications and open it. macOS refuses the first time: the note below has the way past.
</td>
<td width="33%" valign="top">
<b>2 &nbsp; Allow Input Monitoring</b>
<br>
macOS asks once. Switch Keycaps on under System Settings → Privacy &amp; Security → Input Monitoring, then reopen it.
</td>
<td width="33%" valign="top">
<b>3 &nbsp; Pick a switch and type</b>
<br>
Click the keycap in the menu bar, open Switch Keys and listen as you move down the list.
</td>
</tr>
</table>

> [!NOTE]
> **macOS will say it cannot verify Keycaps.** Keycaps is not notarized by Apple, which requires a paid developer account. To open it the first time, try to open it, then go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway**. On macOS 14 and earlier, right-click the app and choose **Open** instead. The same from Terminal: `xattr -dr com.apple.quarantine /Applications/Keycaps.app`. If you would rather not trust a download, [build it yourself](#build-from-source) with one command.

<br><br>

<div align="center">

<sub>THE SOUNDS</sub>

## Thirteen switches, seven mice

| Linear | Tactile | Clicky | Mice |
| :-- | :-- | :-- | :-- |
| Alpaca | Cherry MX Brown | Alps SKCM Blue | Apple Magic Mouse |
| Gateron Black Ink | Holy Panda | Cherry MX Blue | Logitech G502 X |
| Gateron Red Ink | Topre | Kailh Box Navy | Logitech G502 Lightspeed |
| NovelKeys Cream | | Buckling Spring | Logitech M570 Trackball |
| Cherry MX Black | | | Razer Mamba Elite |
| Turquoise Tealios | | | Corsair M65 Elite |
| | | | Amazon Gaming Mouse |

</div>

There are also three synthetic packs, one per family, which Keycaps renders itself from the measured spectrum of a recording instead of playing it back.

<br><br>

<div align="center">

<sub>QUESTIONS</sub>

## Good to know

</div>

<details>
<summary><b>Why does it need Input Monitoring?</b></summary>
<br>
To play a sound when you press a key in another app, Keycaps has to be told about the key press, and macOS only tells apps that you have allowed under Input Monitoring. It is the narrowest permission that does the job. Unlike Accessibility, it does not let an app control your Mac.
<br><br>
</details>

<details>
<summary><b>Does it record or send what I type?</b></summary>
<br>
No. Keycaps keeps a single number, today's count of key presses, in its preferences, and it contains no network code. The details are under <a href="#it-listens-for-keys-it-keeps-none-of-them">privacy</a>.
<br><br>
</details>

<details>
<summary><b>It asks for Input Monitoring again after an update</b></summary>
<br>
That is expected for now. Keycaps is not signed with an Apple Developer ID, so macOS treats every version as a new app. The first time a new version starts, Keycaps clears its old entry, macOS asks again and you allow it once more.
<br><br>
If Keycaps ever stays silent although its switch is on, remove it from the Input Monitoring list with the − button, or run <code>tccutil reset ListenEvent local.keycaps</code>, and open it again.
<br><br>
</details>

<details>
<summary><b>The sound comes from the Mac's speakers although I am wearing headphones</b></summary>
<br>
By design in 1.0. Keycaps always plays through the Mac's built-in speakers, where the sound of a keyboard would come from. On a Mac whose speakers it does not recognise, it uses the output that was selected when it started.
<br><br>
</details>

<details>
<summary><b>Will it slow my Mac down?</b></summary>
<br>
No. At rest it uses no CPU and causes no wake-ups, and while you type it stays under half a percent. The numbers are under <a href="#built-to-cost-nothing">Built to cost nothing</a>.
<br><br>
</details>

<details>
<summary><b>Does it work in every app?</b></summary>
<br>
Yes, including full-screen apps. It is silent in password fields and at the login window.
<br><br>
</details>

<details>
<summary><b>Can it start when I log in?</b></summary>
<br>
Add Keycaps under System Settings → General → Login Items.
<br><br>
</details>

<details>
<summary><b>The scroll ticks sound like a click</b></summary>
<br>
They are one. There is no openly licensed recording of a mouse wheel yet, so the tick is a very short button click, and Scroll ticks is off until you turn it on.
<br><br>
</details>

<details>
<summary><b>Which Macs does it run on?</b></summary>
<br>
Any Mac on macOS 11 or later. Keycaps is developed and tested on Apple silicon with macOS 26. The Intel build passes the same tests under Rosetta, but has not been run on an Intel Mac yet: reports are welcome.
<br><br>
</details>

<details>
<summary><b>How do I remove it?</b></summary>
<br>
Quit Keycaps from its menu and move it to the Trash. To remove its settings as well, run <code>defaults delete local.keycaps</code>. Its entry under Input Monitoring goes with the − button.
<br><br>
</details>

<details>
<summary><b>Is there a Linux version?</b></summary>
<br>
Not yet. It is what comes next.
<br><br>
</details>

<br><br>

## Build from source

You need [Rust](https://rustup.rs) and Apple's command line tools (`xcode-select --install`).

```sh
git clone https://github.com/Walid-Idrissi-Labs/KeyCaps.git
cd KeyCaps
./build.sh && open Keycaps.app
```

`cargo test` runs the tests. `./build.sh release` builds the disk image that the releases carry, and [DISTRIBUTION.md](DISTRIBUTION.md) describes how a release is made.

<table>
<tr><td><a href="src/main.rs"><code>src/main.rs</code></a></td><td>The menu, the settings, the keyboard hook and which sound each key plays</td></tr>
<tr><td><a href="src/audio.rs"><code>src/audio.rs</code></a></td><td>Decoding, trimming and levelling the recordings, and the mixer on the audio thread</td></tr>
<tr><td><a href="src/synth.rs"><code>src/synth.rs</code></a></td><td>The synthetic packs</td></tr>
<tr><td><a href="src/objc.rs"><code>src/objc.rs</code></a></td><td>The few Objective-C runtime calls the menu needs</td></tr>
<tr><td><a href="src/stats.rs"><code>src/stats.rs</code></a></td><td>Today's key count</td></tr>
<tr><td><a href="sounds"><code>sounds</code></a></td><td>The recordings, with their sources and licences</td></tr>
</table>

## Credits and licence

Keycaps is free software under the [GNU General Public License, version 3](LICENSE). Copyright © 2026 Walid Idrissi.

The recordings are other people's work, used under the MIT licence:

- The switches come from [kbsim](https://github.com/tplai/kbsim) by Thomas Lai ([licence](sounds/LICENSE.md)).
- The mice come from [keyboardsounds-pro](https://github.com/keyboard-sounds/keyboardsounds-pro) by Nathan Fiscaletti ([licence](sounds/mouse/LICENSE)), most of them by way of [MechVibesPlusPlus](https://github.com/PyroCalzone/MechVibesPlusPlus) by PyroCalzone ([licence](sounds/mouse/LICENSE-MechVibesPlusPlus)).

Keycaps was inspired by [Kiiwi](https://kiiwi.app).

The names of switches and mice belong to their makers and only say what was recorded. Keycaps is not affiliated with any of them.

<br>

<div align="center">

<img src="assets/icons/industrial/Keycaps-Titanium-K.iconset/icon_128x128.png" width="64" height="64" alt="">

**Now type something.**

<a href="https://github.com/Walid-Idrissi-Labs/KeyCaps/releases/latest/download/Keycaps.dmg"><img src="assets/readme/download.svg" height="48" alt="Download for macOS"></a>

</div>
