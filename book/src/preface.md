# Preface — who this is for

You have a recording that sounds bad. There's a hiss behind the voice, or a low
hum, or it crackles, or someone's "s" sounds slice your ears. You've heard that
software can "fix" this, and maybe you've seen people open intimidating programs
full of knobs and coloured graphs and somehow make it better. You'd like to
understand what's actually happening — without going back to school for it.

This book is for you. **It assumes you know nothing about audio software (a
"DAW") or signal processing ("DSP").** There is no maths you need to follow.
Every idea is explained the way you'd explain it to a curious friend, with
pictures-in-words and everyday analogies.

It's organised around a small open-source toolkit called **cathar** — a program
that cleans up audio. Cathar is a good teaching companion for two reasons.
First, it does one clear job per tool: one button removes hiss, another removes
hum, another rebuilds clipped sound, and so on — so each chapter can be about one
honest idea. Second, cathar is *transparent*: every step it takes is plain,
inspectable arithmetic rather than a secret black box, which means we can
actually say what it's doing.

But this is **not** a manual for cathar. It's a book about the *concepts* that
all audio-restoration tools share. Whether you end up using cathar, or
iZotope RX, or Adobe Audition, or the noise-reduction button in free Audacity,
the underlying ideas are the same — and once you understand the ideas, every one
of those tools stops being mysterious. So at the end of most chapters there's a
short section called **"How the big tools do it"** that lines cathar up against
the professional and free software the rest of the world uses, and tells you,
honestly, where cathar is comparable and where the expensive tools pull ahead.

A note on honesty: audio restoration is *repair*, not magic. Some damage can be
made nearly invisible; some can only be softened; and a few things, once lost,
are gone for good. A good engineer knows the difference, and by the end of this
book so will you.

Let's start with the most basic question of all: what *is* a sound, once it's
inside a computer?
