#!/usr/bin/env python3
"""End-to-end DOWNLOAD test of zoinks on sites that successfully probed.

Downloads the smallest video format to /tmp/zoinks-e2e-dl/, verifies the
file is non-empty, and cleans up. This proves the full pipeline works
end-to-end, not just the probe step.
"""
import os
import sys
import time
from pathlib import Path

from zoinks import Zoinks

OUT_DIR = Path("/tmp/zoinks-e2e-dl")
OUT_DIR.mkdir(parents=True, exist_ok=True)

# (platform, url) — only sites that successfully probed
TESTS = [
    ("X/Twitter",  "https://x.com/techynyxa/status/2106970928504856825"),
    ("Facebook",   "https://www.facebook.com/reel/2135249364543230"),
    ("Soundcloud", "https://soundcloud.com/dualipa/dont-start-now"),
    ("Streamable", "https://streamable.com/moo"),
]

def truncate(s, n):
    return s if len(s) <= n else s[:n-1] + "…"

def main():
    z = Zoinks()
    z.ensure_ytdlp()
    print(flush=True)

    print(f"{'PLATFORM':<13} {'URL':<55} {'TIME':<8} {'SIZE':<10} RESULT", flush=True)
    print("-" * 130, flush=True)

    results = []
    for platform, url in TESTS:
        print(f"{platform:<13} {truncate(url, 55):<55}", end="", flush=True)
        try:
            info = z.probe(url)
            choices = z.build_choices(info)
            video_choices = [c for c in choices if c.kind == "video"]
            audio_choices = [c for c in choices if c.kind == "audio"]
            # pick the smallest: last video (lowest res) if any, else first audio
            choice = (video_choices or audio_choices)[-1]
            t0 = time.time()
            filepath = z.download(
                url,
                choice,
                out_dir=str(OUT_DIR),
                info_json_path=info.info_json_path,
                on_progress=lambda p: None,
                on_processing=lambda: None,
            )
            elapsed = time.time() - t0
            size = os.path.getsize(filepath) if os.path.exists(filepath) else 0
            if size > 0:
                print(f" {elapsed:6.1f}s {size//1024:>6}KB  OK · {truncate(info.title, 50)}", flush=True)
                results.append((platform, url, "OK", size))
                try:
                    os.remove(filepath)
                except Exception:
                    pass
            else:
                print(f" {elapsed:6.1f}s {'0':>6}KB  EMPTY", flush=True)
                results.append((platform, url, "EMPTY", 0))
        except Exception as e:
            msg = str(e).split("\n")[0][:60]
            print(f" {'FAIL':>8} {'':>6}KB  {msg}", flush=True)
            results.append((platform, url, "FAIL", 0))

    print(flush=True)
    print("=" * 130, flush=True)
    ok = sum(1 for r in results if r[2] == "OK")
    print(f"  download success: {ok}/{len(results)}", flush=True)
    print(flush=True)
    for platform, url, status, size in results:
        marker = "✓" if status == "OK" else "✗"
        print(f"  {marker} [{platform}] {truncate(url, 60)} → {status} ({size//1024}KB)", flush=True)

    return 0 if ok == len(results) else 1

if __name__ == "__main__":
    sys.exit(main())
