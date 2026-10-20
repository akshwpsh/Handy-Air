# Audio may leave the device, but only when the user opts in

Handy's core promise is that recorded audio never leaves the machine: every
supported engine runs a model file locally. That promise costs users with
low-end hardware the ability to use Handy at all, because loading a
whisper-family model needs 1-3 GB of RAM and inference is slow enough to make
dictation unpleasant.

We therefore added the **Transcription API**: an opt-in path that uploads
recorded audio to an OpenAI-compatible `/audio/transcriptions` endpoint and
receives text back. It is off by default and never engages without the user
explicitly enabling it and supplying an endpoint and key.

This is a deliberate deviation from the local-only guarantee, not an oversight.
Note the asymmetry with Post-processing, which has always been networked: that
step sends only _text_, whereas this one sends the **recorded voice itself**.
Any future change here should be weighed against that difference.

## Consequences

The default experience is unchanged — a user who never opens this setting is
still fully local. But "Handy never sends your audio anywhere" is no longer
true unconditionally, and any user-facing claim to that effect must now be
qualified.
