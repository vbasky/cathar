# Stereo, channels, and phase

So far we've mostly imagined a single stream of samples — **mono**, one
microphone's worth of sound. But most recordings you meet are **stereo**: two
streams, one for the left ear and one for the right. A couple of ideas about how
those two streams relate will save you from some surprisingly common mistakes.

## Two channels make a space

Your brain locates sounds partly by comparing what your two ears hear. A sound a
little louder and a hair earlier in the left ear is heard as "over to the left."
Stereo recording recreates this: by capturing two slightly different versions of
the scene, it lets your ears reconstruct a **stereo image** — a sense of width
and placement, of instruments spread across a stage.

A **mono** file is just one channel; a player sends it equally to both speakers,
so it sits dead centre. A **stereo** file is two channels, and the *difference*
between them is what creates the width. Keep that word — *difference* — in mind;
it's the whole point of the next two sections.

## A small trap: mono tagged as "left"

Here's a real-world gotcha that bites people constantly. A mono file is supposed
to play equally from both speakers. But the file format has a little label saying
which speaker each channel belongs to, and if a tool mislabels a mono file's one
channel as "front-left" instead of "centre/mono," some players will dutifully send
it **only to the left speaker** — and you'll swear something is broken, even though
the sound itself is perfectly fine and centred.

The audio is balanced; only the *label* is wrong. (Cathar had exactly this bug
once: its mono files were tagged "front-left" and played one-sided until the label
was corrected to "centre.") The lesson for you: if a mono file suddenly plays out
of one speaker, suspect the *channel label*, not the audio — it's a metadata
problem, not a damaged recording.

## Why phase matters when you process stereo

Now the subtle one. Suppose you run a *reducer* — say a denoiser — on a stereo
file. The obvious approach is to clean the left channel and the right channel
**separately**. The hidden danger: the tool might decide a faint pitch is "noise"
in the left channel but "keep it" in the right, on the very same instant. Now the
two channels disagree about that pitch — and remember, the stereo image *is* the
difference between the channels. So the background, the room, the "air" of the
recording starts to **wander and smear** between the speakers as the tool makes
different choices left and right. Engineers call this losing **phase coherence**,
and it makes a cleaned stereo recording sound oddly unstable and "swirly" even
when each channel sounds fine on its own.

The cure is to make the decision **once**, jointly, and apply it to both channels
identically — so the channels always agree about what to keep and what to remove,
and the stereo image stays rock-solid. Cathar offers this as a *phase-coherent*
mode (`denoise --coherent`): it works out one cleaning decision from the combined
("mid") signal and applies that single decision to left and right together. The
image stops wandering.

## How the big tools do it

- Serious restoration and mastering tools are careful about stereo by default.
  **iZotope RX** processes with stereo coherence in mind and offers mid/side and
  linked-channel options throughout; mastering suites like **Ozone** and
  **FabFilter** plug-ins expose mid/side processing explicitly.
- The **mid/side** concept — treating a stereo signal as its "centre" (mid) and
  its "difference" (side) rather than as left/right — is a standard professional
  technique for exactly the reason above: it lets you process the shared centre
  and the stereo width separately and coherently.

Stereo handling is one of those quiet quality markers that separates a tool that
"works" from one that's *trustworthy* on real material. The concepts — width lives
in the *difference*, and processing should keep the channels *agreeing* — are the
same whether you're in cathar, RX, or a full mastering chain.
