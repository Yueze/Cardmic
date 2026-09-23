# Hold to talk (voice keyboard)

Cardmic can act as a push-to-talk key for dictation apps. Hold **Space** on
the Cardputer and the computer sees a key chord held down; release it and the
chord is released. Dictation apps that support a hold-to-talk shortcut start
listening while it is held, and the audio comes from Cardmic itself.

This works over **USB**, where the Cardputer is both the microphone and a
small keyboard.

## Set it up

1. In Cardmic, press `S`, go to **Talk key** and press `Enter` to pick a
   chord. The USB connection restarts once, now as microphone + keyboard.
2. In your dictation app, set the hold-to-talk shortcut to the same chord,
   and pick **Cardmic Microphone** as its microphone.
3. On Cardmic's main screen, hold `Space` to talk. The key cap in the corner
   lights up and the screen says **TALK**.

| Setting | Sends | Good for |
|---|---|---|
| `CTRL+OPT` | Control + Option | macOS apps that accept a modifier-only shortcut |
| `CTRL+WIN` | Control + Windows | Windows apps that accept a modifier-only shortcut |
| `F13` | F13 | Any app that lets you record a custom key; no regular keyboard has it, so it never clashes |
| `OFF` | nothing | Microphone only (default) |

Modifier-only chords keep working in password fields and other secure input,
where macOS blocks ordinary key events.

## Notes

- **macOS may open Keyboard Setup Assistant** the first time the keyboard
  appears. Close it; nothing needs identifying.
- The key cap shows `USB` instead of `TALK` when there is no USB keyboard
  link (for example, on Wi-Fi only).
- The chord is always released when you leave the main screen or close
  Cardmic, so it can never stay stuck.
- With a talk key on, the device uses a different USB product ID (`0x4016`
  instead of `0x4015`), so computers treat mic-only and mic+keyboard as two
  devices and do not mix up their cached drivers.
