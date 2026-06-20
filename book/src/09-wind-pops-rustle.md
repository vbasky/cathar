# Wind, pops, and rustle

Three more everyday nuisances, all caused by *physical things hitting the
microphone* rather than by electronics or rooms. They share a theme, which is why
they're together: each is a burst of unwanted energy concentrated in a particular
part of the spectrum, and each is removed by acting on that part — sometimes all
the time, sometimes only during the burst.

## Wind — the low rumble

Record outdoors without a foam or furry "dead-cat" cover and the breeze
turbulating across the mic produces a low, blustery **rumble** — sometimes a
roar. Crucially, almost all of that energy sits **very low**, below the range
where speech lives.

That makes the cure simple: a **high-pass filter** — a tool that lets the high
stuff *pass* and blocks the low stuff. Set its cutoff at, say, 80 cycles per
second and everything below (the wind rumble) is steeply rolled off while the
voice above is untouched. Cathar's `dewind --cutoff 80` is exactly this, built
from a classic, very steep filter shape (a *Butterworth*) so the rumble drops
away fast without disturbing the voice just above it. It's the same high-pass you
hear engineers reach for the instant an outdoor clip starts rumbling.

## Plosives — the "p" thumps

Get close to a mic and say "**p**eter **p**iper" and each "p" and "b" fires a
little puff of air straight at the capsule, producing a low **thump** — a
*plosive*. Like wind, a plosive is mostly low-frequency energy — but unlike wind,
it's not constant: it's a brief burst, only on the plosive consonants.

So instead of filtering all the time, **de-plosive** watches the low end and
acts *only when it suddenly thumps*: it spots the short bursts of excess low
energy and ducks just those moments, leaving the steady low warmth of the voice
in between alone. (The physical prevention, by the way, is the round foam ball or
mesh "pop filter" you've seen in front of studio mics — but when you're handed a
recording that already has the thumps, software has to clean up after the fact.)

## Rustle — the clip-on-mic scratch

The little clip-on (*lavalier*) mics used in interviews and film sit against
clothing, and every time the wearer shifts, the fabric scrapes the mic and makes a
scratchy **rustle**. This one is sneakier: it's a brief burst like a plosive, but
it lands in the *mid* range, right among the consonants of speech, so you can't
just filter it away without dulling the voice.

**De-rustle** therefore does the same "act only during the burst" trick as
de-plosive, but aimed at the mid-range: it watches a band roughly where rustle
lives (around 1,500–6,000 cycles) and, when energy there *spikes briefly above
its normal level*, it pulls just that fleeting spike back down, while sustained
speech in the same band passes through. It's the hardest of the three, because the
rustle and the wanted consonants are near neighbours.

## How the big tools do it

- The **high-pass for wind** is utterly universal — every DAW channel strip,
  every mixer, has a low-cut button. Nothing exotic here, in cathar or anywhere.
- For **plosives**, engineers often just automate a quick low-cut on the offending
  word, or use a dynamic filter; **iZotope RX's** "De-plosive" automates exactly
  the spot-the-thump-and-duck-the-lows behaviour cathar uses.
- **Rustle** is genuinely hard, and it's a showcase for ML: **iZotope RX's**
  "De-rustle" was one of the first ML-driven restoration modules precisely because
  fabric noise overlaps speech so much that a learned model separates them far
  better than a rule about bands. Cathar's transient-suppression approach gives a
  useful reduction on obvious rustles; deep, speech-tangled rustle is RX's
  territory.

Notice the recurring pattern across this whole section: a *steady* offender
(wind) gets a *filter that's always on*; a *bursty* offender (plosive, rustle)
gets a *watcher that acts only during the burst*. That single distinction — always
on versus only-when-it-happens — explains an enormous amount of audio software.
