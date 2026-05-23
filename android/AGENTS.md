# Bark

Voice input app for android.

Target behaviour:
- Keyboard opens -> recording starts
- Recording stops -> switch to previous keyboard
- Recording: 16kHz 16‑bit mono OGG/Opus
- Transcription: HTTP multipart upload to configurable endpoint

## VAD

Silero VAD is used for (1) silencing non-voice segments and (2) truncating silence.

1. Replace all non-voice segments with silence.
2. All non-voice segments >500ms should be truncated to 500ms.
