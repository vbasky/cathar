# How cathar compares to the big tools

You now understand the concepts. This chapter steps back and places cathar — and
the ideas in this book — next to the software the rest of the world uses, so you
know which tool fits which job. The goal isn't to crown a winner; it's to make you
a clear-eyed chooser.

## The landscape, in plain terms

Audio software for cleanup falls into a few camps:

- **The restoration specialist — iZotope RX.** The industry standard for cleaning
  up dialogue, podcasts, music, and archival audio. Spectrogram-centred, deep,
  increasingly powered by machine learning. Expensive, and worth it for people
  who do this for a living.
- **The all-rounders — Adobe Audition, and DAWs (Pro Tools, Logic, Ableton,
  Reaper, Cubase).** Audio editors and studios that *include* restoration tools
  alongside everything else (recording, mixing, effects). Good, not always
  best-in-class for restoration.
- **The free editor — Audacity.** Free and open-source, with genuinely useful
  noise reduction, click removal, and filters. The place millions of people first
  clean up a recording.
- **The command-line workhorses — SoX and FFmpeg.** No window, no buttons — you
  type a command. Beloved for *batch* work and automation: converting,
  resampling, loudness-normalizing thousands of files. FFmpeg in particular
  quietly powers a huge fraction of the internet's media processing.
- **cathar.** A small, open-source, command-line-and-library toolkit in pure Rust
  — squarely in the SoX/FFmpeg "workhorse" camp by *form*, but focused on
  *restoration* like RX by *intent*.

## What makes cathar different

Three deliberate choices define it:

1. **Pure, self-contained, no dependencies on the usual giants.** Most audio tools
   lean on big C/C++ libraries (often FFmpeg) under the hood. Cathar is written
   entirely in Rust and carries its own decoding, maths, and encoding — one
   build, one self-contained program, nothing to install alongside it.
2. **No black boxes.** Every stage is plain, inspectable arithmetic — the exact
   methods this book describes. There are no trained neural-network weights making
   unexplainable decisions. If you don't like a result, it's a *knob you can turn*,
   with an understandable reason, rather than a model you have to re-roll and hope.
3. **One clear job per tool, scriptable.** Like SoX, it's built to be driven from
   the command line and dropped into automated pipelines — clean a thousand files
   the same way, reproducibly.

## What that buys you — and what it costs

Be honest about both sides:

**Where cathar holds its own.** The *settled-science* tasks — loudness (LUFS /
EBU R128 / true-peak), resampling, de-hum (fixed or `--adaptive`), de-essing,
steady-state hiss reduction, RIAA/FM/CD de-emphasis, declick/declip, wow/flutter
and azimuth on analog transfers, HPSS separation, short-gap inpainting, and
batchable time/pitch edits — are well-defined problems with known classical
methods, and cathar implements many of them properly. For these, it's genuinely
comparable to the big tools on moderate material, with the bonus of being
transparent and automatable. If your job is "batch-normalize 500 podcast episodes
to −16 LUFS, notch 60 Hz hum, and de-reverb dialogue with WPE," cathar is the
right shape of tool.

**Where the expensive tools pull ahead.** The *hard, perceptual* tasks — heavy or
non-steady noise (a busy café behind a voice), film-grade de-reverberation on
impossible rooms, fabric rustle tangled in speech, badly clipped material — are
where **machine learning** has changed the game. iZotope RX's learned models
separate sound from noise in ways no "subtract the haze" or "gate the tails"
rule can match. Cathar's optional `ml-denoise` closes *some* of that gap on
speech, but RX-class models on the worst cases are still ahead. For professional
film, broadcast, and archival restoration of *difficult* material, RX is the
benchmark. Cathar's classical (and selectively learned) methods give a real,
useful improvement on moderate problems; they are not a blanket substitute for a
trained model on every nightmare file.

## A cheat-sheet for choosing

| If you need to… | Reach for |
| --- | --- |
| Clean difficult dialogue for film/broadcast | **iZotope RX** |
| Clean up a recording inside a project you're already editing | your **DAW** or **Audition** |
| Quickly de-noise/de-click one file, for free, with a GUI | **Audacity** |
| Batch-convert, resample, or loudness-normalize many files | **FFmpeg / SoX / cathar** |
| Batch *restoration* (de-hum, de-noise, de-reverb/WPE, vinyl chain, inpaint, loudness) in a script or pipeline, transparently, in pure Rust | **cathar** |
| Understand, embed, or extend the actual DSP in your own program | **cathar** (it's a library too) |

## The real takeaway

The most valuable thing this book gives you isn't a verdict on cathar — it's that
**every one of these tools runs on the same handful of ideas.** Analyse into
pitches, modify, resynthesise. Subtract the haze. Notch the hum. Redraw the click.
Predict the clipped peak. Gate the reverb tail. Measure loudness the way ears
hear. Filter, don't copy, when you resample. Keep the stereo channels agreeing.

Once those ideas are yours, no audio program is a black box anymore — including
the ones that cost a fortune. You'll open RX or Audacity or a DAW, see a panel of
knobs, and *know what they must be doing*, because there are only so many honest
ways to clean up a sound. That understanding outlasts any one tool.
