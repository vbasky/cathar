#!/usr/bin/env python3
"""
Train a GRU spectral-gain denoiser matching cathar's NeuralDenoiser architecture.

Produces a `.safetensors` checkpoint loadable by
`NeuralDenoiser::from_safetensors(path, NeuralConfig::default())`.

The model is small (~528K params) and trains on synthetic clean + noisy pairs
so it learns to suppress stationary and modulated noise. For production use,
retrain on DNS-Challenge (https://github.com/microsoft/DNS-Challenge) speech.

Usage:
    pip install torch safetensors
    python scripts/train_denoiser.py [--epochs 40] [--out denoiser.safetensors]
"""

import argparse
import math
import os
import random
import time

import torch
import torch.nn as nn
import torch.nn.functional as F
import torch.optim as optim
from safetensors.torch import save_file


# ── config ──────────────────────────────────────────────────────────────────

N_BINS = 257
HIDDEN = 256
FFT_SIZE = 512
HOP_SIZE = 128
SAMPLE_RATE = 48000
FRAMES_PER_EXAMPLE = 100
BATCH_SIZE = 32
LEARNING_RATE = 1e-3
N_EXAMPLES = 2000


# ── model (exactly matches cathar::ml::Model) ───────────────────────────────

class DenoiserModel(nn.Module):
    def __init__(self):
        super().__init__()
        self.enc = nn.Linear(N_BINS, HIDDEN)
        self.gru = nn.GRU(HIDDEN, HIDDEN, batch_first=True)
        self.dec = nn.Linear(HIDDEN, N_BINS)

    def forward(self, log_mag):
        x = F.relu(self.enc(log_mag))
        x, _ = self.gru(x)
        return torch.sigmoid(self.dec(x))


# ── data generation (pre-generated for speed) ───────────────────────────────

def make_example():
    """Create one clean+noisy pair of log-magnitude spectra."""
    n_samples = FRAMES_PER_EXAMPLE * HOP_SIZE + FFT_SIZE
    sr = SAMPLE_RATE

    # Clean: mix of tones simulating formants/harmonics
    clean = torch.zeros(n_samples)
    f0 = random.uniform(80, 400)
    t = torch.arange(n_samples, dtype=torch.float32) / sr
    for h in range(random.randint(2, 6)):
        amp = 0.3 * (0.6 ** h) * random.uniform(0.7, 1.3)
        freq = f0 * (h + 1) + random.uniform(-5, 5)
        clean += amp * torch.sin(2 * math.pi * freq * t)
    clean += 0.2 * torch.sin(2 * math.pi * f0 * t + random.uniform(0, math.pi))

    # Amplitude envelope for non-stationarity
    envelope = torch.ones(n_samples)
    for _ in range(random.randint(2, 5)):
        pos = random.randint(0, n_samples - 1)
        width = random.randint(n_samples // 8, n_samples // 3)
        envelope += random.uniform(0.3, 1.2) * torch.exp(
            -((torch.arange(n_samples, dtype=torch.float32) - pos) / width) ** 2
        )
    clean *= envelope / 3.0

    peak = clean.abs().max()
    if peak > 0:
        clean /= peak * 1.2

    # Noise: white + modulated
    noise = torch.randn(n_samples)
    noise += 0.3 * torch.sin(
        2 * math.pi * random.uniform(30, 120) * torch.arange(n_samples) / sr
    ).squeeze() * torch.randn(1).item()
    mod = 1.0 + 0.5 * torch.sin(
        2 * math.pi * random.uniform(0.5, 3.0) * torch.arange(n_samples) / sr
    )
    noise = (noise * mod)
    noise = noise / noise.std() * 0.15 * random.uniform(0.5, 2.0)

    noisy = clean + noise

    # STFT
    window = torch.hann_window(FFT_SIZE)
    clean_stft = torch.stft(clean, FFT_SIZE, HOP_SIZE, window=window, return_complex=True)
    noisy_stft = torch.stft(noisy, FFT_SIZE, HOP_SIZE, window=window, return_complex=True)

    # Transpose to (frames, bins)
    clean_mag = clean_stft.abs().t()
    noisy_mag = noisy_stft.abs().t()

    T = min(clean_mag.shape[0], FRAMES_PER_EXAMPLE)
    clean_mag = clean_mag[:T]
    noisy_mag = noisy_mag[:T]

    feat = torch.log(noisy_mag + 1e-6)
    target = (clean_mag / (noisy_mag + 1e-8)).clamp(0.0, 1.0)

    if feat.shape[0] < FRAMES_PER_EXAMPLE:
        pad = FRAMES_PER_EXAMPLE - feat.shape[0]
        feat = F.pad(feat, (0, 0, 0, pad))
        target = F.pad(target, (0, 0, 0, pad))

    return feat[:FRAMES_PER_EXAMPLE], target[:FRAMES_PER_EXAMPLE]


def generate_dataset(n):
    """Pre-generate N examples so we don't STFT every epoch."""
    feats, targets = [], []
    for i in range(n):
        f, t = make_example()
        feats.append(f)
        targets.append(t)
        if (i + 1) % 500 == 0:
            print(f"  generated {i + 1}/{n} examples...")
    return torch.stack(feats), torch.stack(targets)


# ── train ───────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Train cathar denoiser model")
    parser.add_argument("--epochs", type=int, default=30)
    parser.add_argument("--out", default="denoiser.safetensors")
    parser.add_argument("--device", default="cpu")
    args = parser.parse_args()

    device = torch.device(args.device)

    print(f"Generating {N_EXAMPLES} training examples...")
    t0 = time.time()
    X, Y = generate_dataset(N_EXAMPLES)
    print(f"  done in {time.time() - t0:.0f}s  shape: {tuple(X.shape)}")

    model = DenoiserModel().to(device)
    optimizer = optim.Adam(model.parameters(), lr=LEARNING_RATE)
    scheduler = optim.lr_scheduler.CosineAnnealingLR(optimizer, args.epochs)
    loss_fn = nn.MSELoss()

    n_batches = N_EXAMPLES // BATCH_SIZE
    n_params = sum(p.numel() for p in model.parameters())

    print(f"\nTraining {n_params:,} params on {device}")
    print(f"  input: {N_BINS} bins  hidden: {HIDDEN}  frames: {FRAMES_PER_EXAMPLE}")
    print(f"  {n_batches} batches/epoch  LR: {LEARNING_RATE}\n")

    best_loss = float("inf")
    t0 = time.time()
    indices = torch.arange(N_EXAMPLES)

    for epoch in range(1, args.epochs + 1):
        model.train()
        total_loss = 0.0
        perm = torch.randperm(N_EXAMPLES)

        for i in range(0, N_EXAMPLES, BATCH_SIZE):
            idx = perm[i : i + BATCH_SIZE]
            feat = X[idx].to(device)
            target = Y[idx].to(device)
            pred = model(feat)
            loss = loss_fn(pred, target)
            optimizer.zero_grad()
            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()
            total_loss += loss.item()

        scheduler.step()
        avg_loss = total_loss / n_batches
        if avg_loss < best_loss:
            best_loss = avg_loss

        elapsed = time.time() - t0
        print(
            f"epoch {epoch:3d}/{args.epochs}  "
            f"loss={avg_loss:.6f}  best={best_loss:.6f}  "
            f"LR={scheduler.get_last_lr()[0]:.2e}  "
            f"{elapsed:.0f}s"
        )

    print(f"\nDone. Best loss: {best_loss:.6f}")

    # ── export ──────────────────────────────────────────────────────────────
    state = {
        "enc.weight": model.enc.weight.data.contiguous().cpu(),
        "enc.bias": model.enc.bias.data.contiguous().cpu(),
        "gru.weight_ih_l0": model.gru.weight_ih_l0.data.contiguous().cpu(),
        "gru.weight_hh_l0": model.gru.weight_hh_l0.data.contiguous().cpu(),
        "gru.bias_ih_l0": model.gru.bias_ih_l0.data.contiguous().cpu(),
        "gru.bias_hh_l0": model.gru.bias_hh_l0.data.contiguous().cpu(),
        "dec.weight": model.dec.weight.data.contiguous().cpu(),
        "dec.bias": model.dec.bias.data.contiguous().cpu(),
    }

    save_file(state, args.out)
    size_kb = os.path.getsize(args.out) / 1024
    print(f"Saved {args.out} ({size_kb:.0f} KB)")

    # Smoke test
    model.eval()
    with torch.no_grad():
        feat = X[:1].to(device)
        pred = model(feat).squeeze(0)
        mean_gain = pred.mean().item()
        print(f"Inference mean gain: {mean_gain:.4f} (should be < 0.95 for real denoising)")
        # The model should reduce gain meaningfully for noisy bins
        n_suppressed = (pred < 0.8).sum().item()
        print(f"Bins suppressed below 0.8: {n_suppressed}/{pred.numel()}")

    print("Done.")


if __name__ == "__main__":
    main()
