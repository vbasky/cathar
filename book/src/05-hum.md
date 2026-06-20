# Hum — getting rid of the buzz

That low, steady *hummmm* under so many recordings has a single, boring villain:
**the electrical mains.** Wall power doesn't sit still — it alternates back and
forth 50 times a second in most of the world, 60 times a second in the Americas.
Any nearby cable, cheap power supply, or poorly grounded microphone can leak a
little of that alternation into the audio as a pure, relentless tone.

## Why it's actually the easy one

Go back to the spectrogram. Hum is the *simplest possible* picture: a single
razor-thin horizontal line, sitting at exactly 50 (or 60) cycles per second,
present from start to finish. It doesn't move. It doesn't overlap much with the
important parts of a voice. That makes it a sitting duck.

The tool for a single unwanted pitch is a **notch filter** — think of it as a
very narrow pair of scissors that snips out one exact frequency and leaves
everything on either side untouched. Tell it "remove 60 cycles per second" and it
carves a thin notch there, killing the hum while the voice just above and below
sails through.

## The harmonics catch

There's one wrinkle that trips up beginners. Mains hum is rarely *just* the base
tone. The same electrical leakage usually brings along faint copies at exact
multiples: 120, 180, 240… (for a 60-cycle hum), or 100, 150, 200… (for 50). These
copies are called **harmonics**, and they're why hum often sounds more like a
"buzz" than a pure tone — your ear hears the whole stack.

So a hum remover doesn't place one notch; it places a **comb** of them — one at
the base frequency and one at each harmonic up the spectrum.

```text
  loudness
     │  voice and music live in the gaps — untouched
     │   ▁▂▃▅▇█▇▅▃▂▁    ▁▂▃▅▇▇▅▃▂▁     ▁▂▃▅▅▃▂▁
     └──┬─────┬─────┬─────┬─────┬─────┬──────►  pitch
       60    120   180   240   300   360  Hz
        V     V     V     V     V          ← one narrow "notch" snipped at the
       hum  +harmonics (exact multiples)      hum and each of its harmonics
```

In cathar you say `dehum --freq 60 --harmonics 5`, and it snips 60, 120, 180,
240, and 300 cycles.
If a hum still buzzes after you remove the base tone, you simply haven't notched
enough of its harmonics.

> **The 50 vs 60 gotcha:** if `--freq 60` doesn't help, try `--freq 50`. A
> recording made in Europe, most of Asia, Africa, or Australia will hum at 50; the
> Americas and parts of Japan at 60. Guessing wrong does nothing, because the
> notch lands between the hum's actual lines.

## How the big tools do it

Once again the concept is universal — narrow notches at the fundamental and its
harmonics — and the tools differ mostly in convenience:

- **Audacity** has a "Notch Filter" effect (you place them by hand) and the
  free **"Hum Removal"** and Nyquist plug-ins that automate the comb.
- **Adobe Audition's** "DeHummer" gives you a tidy panel: pick 50 or 60, choose
  how many harmonics, done — exactly cathar's two controls with a nicer face.
- **iZotope RX's** "De-hum" adds a smart twist: real mains hum *drifts* a tiny
  bit as the power grid fluctuates, and the line isn't perfectly stable. RX can
  *track* that drift and follow the hum, and can learn the exact harmonic
  fingerprint of a particular buzz. Cathar's notches are fixed in place, which is
  perfectly fine for steady hum but can leave a little residue if the hum wanders.

This is the rare corner of audio where the cheap and free tools are genuinely
*close* to the expensive ones, because the problem is so well-defined. Hum is the
friendliest enemy in this whole book.
