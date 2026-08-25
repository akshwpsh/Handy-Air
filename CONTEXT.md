# Handy

Handy turns speech into text and types it into whichever application has focus.
This document defines the vocabulary used across the codebase and in design
discussions. It is a glossary only — no implementation details belong here.

## Language

### Transcription

**Transcription**:
The conversion of recorded audio into text. Produces the raw words a user
dictated, before any cleanup.
_Avoid_: STT, speech recognition, dictation (as a noun for the output)

**Local Engine**:
A transcription path that runs a model file on the user's own machine.
_Avoid_: offline mode, built-in model, native engine

**Transcription API**:
A transcription path that sends audio to an OpenAI-compatible
`/audio/transcriptions` endpoint outside this process and receives text back.
The endpoint is user-configurable, so it may be a hosted service or a server on
the user's own network — the term deliberately claims nothing about location.
_Avoid_: cloud transcription, remote STT, Groq transcription

**Local Fallback**:
Falling back to the Local Engine when the Transcription API fails. Opt-in, and
off by default: a user who chose the Transcription API usually did so because
the Local Engine is too slow on their machine.
_Avoid_: offline fallback, retry

**Provider**:
A configured combination of endpoint base URL, API key, and model name. Handy
configures Providers separately for Transcription and for Post-processing; they
need not be the same service.
_Avoid_: backend, service, vendor

### Text handling after transcription

These two are separate steps with confusingly similar names in the code. The
distinction matters: one is local and always on, the other is networked and
opt-in.

**Text Filtering**:
The on-device cleanup applied to every transcription — custom-word correction
and filler-word removal. Runs regardless of which transcription path produced
the text.
_Avoid_: post-processing, text post-processing

**Post-processing**:
An optional pass in which an LLM rewrites the transcribed text according to a
user-chosen prompt. Performed by a Provider, which may be a hosted service, a
server on the user's own network, or Apple Intelligence. Never a bundled local
model.
_Avoid_: LLM cleanup, refinement, AI formatting
