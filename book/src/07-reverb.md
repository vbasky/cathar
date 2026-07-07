# Rooms and reverb — taking the echo away

Record someone in a tiled bathroom and they sound like they're in a tiled
bathroom. Record them in a small carpeted booth and they sound "close" and "dry."
The difference is **reverb** — the thousands of tiny echoes a room adds as sound
bounces off the walls, floor, and ceiling before it reaches the mic.

## What reverb really is

When you speak, the mic hears two things. First the **direct sound** — your voice
travelling straight to it. Then, a few thousandths of a second later, a flood of
**reflections** — the same sound arriving again and again, having bounced around
the room, each copy a little quieter and a little later than the last. That trail
of fading echoes is reverb. A big stone hall has a long, obvious trail; a small
treated studio has almost none.

Reverb is the trickiest "reducer" in this book, because the echoes are *made of
the exact same sound as the voice* — they're just delayed, quieter copies. You
can't separate them by pitch the way you separate hiss, because they share the
voice's pitches entirely.

## The trick: watch how each pitch fades

So de-reverb uses *timing* instead of pitch. Here's the insight. When you start a
new word, the direct sound arrives as a sharp **onset** — a quick rise in energy.
Then you stop, but the room keeps ringing: the energy at each pitch **decays
away** in a smooth, tell-tale tail. That decaying tail *is* the reverb.

A de-reverb tool watches each pitch over time and learns the difference between
the punchy onsets (keep these — they're the real voice) and the lingering decay
tails (turn these down — they're the room). In effect it follows the energy at
every pitch and, whenever the energy is just *coasting downward toward the room's
background level*, it gates it back. The direct, intentional sound survives; the
ringing afterglow is suppressed.

Cathar's default **`dereverb`** does exactly this with a two-pass scan: first it
measures how low each pitch typically sinks (the "reverb floor"), then it gently
gates anything sitting near that floor. The `--strength` knob controls how
aggressively it chases the tails.

```bash
cathar dereverb roomy.wav --strength 0.5 --out drier.wav
```

## A deeper mode — WPE

On speech with a **long, smeary tail**, the energy-gating approach can sound
hollow — it knows *how loud* each pitch is, but not how the late echoes relate
to earlier ones. **Weighted Prediction Error (WPE)** is a published blind
de-reverb method that treats each frequency bin separately: late STFT frames are
predicted as a weighted mix of slightly earlier frames at that same pitch, and
the prediction — the reverb part — is subtracted. No noise profile, no trained
model; just linear prediction in the frequency domain.

```bash
cathar dereverb speech.wav --wpe --out drier.wav
```

Use WPE when the room tail is obvious on dialogue and the default gate leaves
too much mush. It's heavier maths than gating, and like all de-reverb it trades
against naturalness if you push too hard — but on moderate roominess it often
pulls voice forward more cleanly than strength-gating alone.

## Why it's never perfect

Two honest limitations:

- **Onsets and tails overlap.** Fast speech starts a new word before the previous
  one's tail has died, so the tool is always making a judgement call, and pushed
  hard it can make a voice sound a bit hollow, gated, or "phasey."
- **You can dry a room but not delete it.** De-reverb shortens and softens the
  trail; it can't put you in a different room. Targeting a modest improvement —
  "less boomy," "a bit closer" — gives far nicer results than chasing total
  removal.

## How the big tools do it

- **iZotope RX's** "De-reverb" is the leader, and the gap here is large. It uses
  a learned model of the reverb tail and, in recent versions, machine learning to
  separate dry voice from room — it can take a startling amount of reverb off a
  voice while keeping it natural. There's a separate "Dialogue De-reverb" tuned
  for speech.
- **Acon Digital** and **Accentize** make well-regarded dedicated de-reverb
  plug-ins used in film post-production, several now ML-based.
- **Audition** has a "DeReverb" effect; **Audacity** has no real built-in
  de-reverb, which tells you how much harder this problem is than hum or hiss.

This is the area where classical, no-model tools like cathar are *most* outclassed
by modern ML, because separating a sound from delayed copies of itself is exactly
the kind of "needs a trained ear" task that a learned model does best. Cathar's
gate-the-tails approach and its `--wpe` predictor both give real, useful reduction
on moderate reverb; for heavy, film-grade de-reverberation, RX is in a different
league.
