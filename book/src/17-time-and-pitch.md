# Time and pitch — stretching without the chipmunk effect

Two of the most-wanted edits sound like they should be the same thing, but
they're opposites:

- **Change the speed** — make a recording play faster or slower.
- **Change the pitch** — make it higher or lower.

On a tape deck or a turntable they're welded together: spin the reel faster and
everything gets shorter *and* higher — the "chipmunk" effect. Digitally we can
pull them apart. Cathar gives you three commands, and the difference between them
is exactly which of those two things you hold still.

```text
  speed  : faster  → shorter AND higher   (like a tape deck)   cathar speed
  tempo  : faster  → shorter, SAME pitch                        cathar tempo
  pitch  : higher  → SAME length                                cathar pitch
```

## Speed — the honest tape deck

`cathar speed --factor 1.5` simply plays 1.5× faster: the file gets shorter and
the pitch rises, exactly like spinning a reel faster. Under the hood it's just
**resampling** — read the samples at a different rate (see the resampling
chapter). It's the right tool when you *want* the tape-deck behaviour, or to nudge
a recording that was captured at a slightly wrong speed.

## Tempo — change the length, keep the pitch

This is the clever one. You want a podcast 10% shorter without everyone sounding
like a chipmunk. The trick is **granular overlap-add**: chop the sound into short
overlapping grains and lay them back down closer together (to speed up) or
farther apart (to slow down). Do that naïvely and you get clicks and warbles at
every join. The fix is to **slide each grain a few milliseconds** so its waveform
lines up with the one before it — the joins become invisible. That method is
called **WSOLA** (waveform-similarity overlap-add), and it's Cathar's default:

```bash
cathar tempo lecture.wav --factor 1.25     # 25% faster, voices unchanged
cathar tempo song.wav    --factor 0.8      # 20% slower
```

For sustained, tonal material (strings, pads) there's a second engine, the
**phase vocoder**, which works in the frequency domain and keeps long notes
smooth. Pick it with `--mode pv`; stick with the default `wsola` for speech and
anything percussive.

## Pitch — change the note, keep the length

Pitch-shifting is tempo and speed working together. To raise the pitch a
semitone without changing the length, Cathar:

1. **time-stretches** the audio longer by the pitch ratio (pitch unchanged), then
2. **resamples** it back down to the original length — which speeds it up, and
   *that* is what raises the pitch.

```bash
cathar pitch vocal.wav --semitones -2      # down a whole tone
cathar pitch vocal.wav --semitones 7       # up a fifth
```

A tone shifted up an octave (`--semitones 12`) comes out exactly twice the
frequency, in the same number of seconds.

## When it sounds artificial

Time-stretching is never free. Push `tempo` or `pitch` past roughly ±20–30% and
you'll start to hear it: a slight metallic "phasiness" on the phase vocoder, or a
faint fluttering on WSOLA. That's the algorithm inventing information that was
never recorded. Small moves are transparent; big ones are an effect, not a
repair. Use the mode that suits the material, and reach for the smallest factor
that gets the job done.
