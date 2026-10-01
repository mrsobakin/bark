<p align="center">
    <img src="bark-logo.svg" alt="🗣" width="96" height="96">
    <img src="bark-logo.svg" alt="🗣" width="96" height="96">
    <img src="bark-logo.svg" alt="🗣" width="96" height="96">
</p>

# Bark

Simple, fast and minimal speech-to-text app.

## Usage

You can download a build from [GitHub Releases](https://github.com/mrsobakin/bark/releases).

The easiest way to use bark is to bind its oneshot mode to some keyboard shortcut (I use <kbd>Insert</kbd>):

```sh
barkd daemon --oneshot
```

Run it once to start recording, then again to stop. Bark will transcribe the recording, type the text into the focused application, and exit.

The default config path is `~/.config/barkd/config.toml` on Linux and `%APPDATA%\barkd\config.toml` on Windows.

## How does this work?

All platforms use `bark-core`, which is a custom transcription pipeline optimized for low latency (end-of-speech to full transcription time). As you feed it audio chunks, it applies preprocessing (AGC, VAD) and encodes the audio stream to opus/ogg. After you end the stream, it sends that encoded & cleaned up audio to your (Whisper-compatible) transcription API of choice, and does basic text postprocessing (normalization and regexes). Groq is set as the default transcription API because it's free and fast.

You'll be surprised, but VAD and encoding can take quite some time on phones, especially if you talk for a long time 😉. That annoying encoding delay was actually the reason why I've decided to move everything to `bark-core` and make the preprocessing realtime. Everything is bundled into it, including VAD inference runtime & model and HTTP client. It's just easier that way and gives a nice internal interface with a unified config.

Still, I don't stream the audio to the transcription API. Basically no transcription engine supports streaming, and saving measly 150ms on the initial TLS connection or whatever just isn't worth the added complexity.

## Can you add LLM postprocessing?

If anybody besides me actually uses bark and wants it, drop an issue and I'll add it.

## My config

```toml
[daemon]
typer = ["wtype", "-"]  # Because I use wayland.

[daemon.recorder]
timeout = 300

[pipeline.engine]
api_key = "gsk_blahblahblah"
model = "whisper-large-v3-turbo"
prompt = ""

[pipeline.pre.agc]
[pipeline.pre.vad]

[[pipeline.post]]
type = "normalize"

[[pipeline.post]]
type = "regex"
pattern = "\\s+"
with = " "

[[pipeline.post]]
type = "regex"
pattern = "\\.$"
with = ""
```
