# The Transcription API is a parallel path, not an engine variant

The obvious place for the Transcription API is alongside the existing engines:
a `Remote` variant of the loaded-engine enum, so every caller treats it like any
other model. We deliberately did not do that.

This repository is a personal fork that intends to keep merging upstream
indefinitely. The engine enum, the model catalog, and the model downloader are
among the files upstream changes most often, and all three assume an engine is a
downloadable file on disk. Threading a network path through them would put our
changes directly in the way of every future merge.

Instead the Transcription API lives in its own module and is reached by a single
early branch at the top of the transcription entry point. Existing files gain
additions — a settings field, a module declaration, one render line in the UI —
but almost no modifications. For the same reason the network call runs on a
dedicated thread with its own runtime rather than making the transcription entry
point async, which would have required editing all three of its callers.

## Consequences

The cost is architectural: there are now two transcription paths that must be
kept in step by hand rather than one abstraction with two implementations. In
particular, anything applied to local results — text filtering, language
handling, translation — must be applied deliberately to the API path too, and
nothing enforces that. This is an accepted trade for cheap upstream merges. If
this fork ever stops tracking upstream, folding the path into the engine
abstraction becomes the right move.
