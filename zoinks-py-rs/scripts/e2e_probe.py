#!/usr/bin/env python3
"""End-to-end test of zoinks against popular video sites.

Just runs probe() + build_choices() — no actual downloads (those would be slow
and write files all over the place). If probe succeeds, the site works in
zoinks. Downloads use the same yt-dlp invocation path, so a probe success is
a strong signal that downloads will work too.

URLs verified to work with the installed yt-dlp version (2026.08.19) BEFORE
running through zoinks — so any failures here are zoinks's fault, not yt-dlp's.
"""
import sys
import time
from pathlib import Path

from zoinks import Zoinks

# (platform, url, note)
TESTS = [
    # verified working directly with yt-dlp — should work through zoinks too
    ("X/Twitter",   "https://x.com/techynyxa/status/2106970928504856825", ""),
    ("X/Twitter",   "https://x.com/elonmusk/status/2106970928504856825",  ""),
    ("Facebook",    "https://www.facebook.com/reel/2135249364543230",     ""),
    ("Soundcloud",  "https://soundcloud.com/dualipa/dont-start-now",      "audio"),
    ("Streamable",  "https://streamable.com/moo",                         ""),

    # cookies-required sites — should give a clean error message, not a hang
    ("YouTube",     "https://youtu.be/EoEQ0kQD2oo",                       "needs cookies for bot detection"),
    ("Vimeo",       "https://vimeo.com/76979871",                         "needs login"),
    ("Reddit",      "https://www.reddit.com/comments/1fxqgq3/",           "needs login"),
    ("Instagram",   "https://www.instagram.com/reel/Cz5xK3qAhXO/",         "needs login"),
]

def truncate(s, n):
    return s if len(s) <= n else s[:n-1] + "…"

def main():
    z = Zoinks()
    print("ensuring yt-dlp is installed...", flush=True)
    ytdlp_path = z.ensure_ytdlp()
    print(f"  yt-dlp at: {ytdlp_path}", flush=True)
    print(flush=True)

    print(f"{'PLATFORM':<13} {'URL':<60} {'TIME':<6} {'RESULT':<8} DETAIL", flush=True)
    print("-" * 130, flush=True)

    results = []
    for platform, url, note in TESTS:
        print(f"{platform:<13} {truncate(url, 60):<60}", end="", flush=True)
        t0 = time.time()
        try:
            info = z.probe(url)
            elapsed = time.time() - t0
            choices = z.build_choices(info)
            title = truncate(info.title or "(no title)", 60)
            print(f" {elapsed:5.1f}s {'OK':<8} {len(choices)} choices · {title}", flush=True)
            results.append((platform, url, "OK", f"{len(choices)} choices · {info.title}", note))
        except Exception as e:
            elapsed = time.time() - t0
            msg = str(e).split("\n")[0][:80]
            print(f" {elapsed:5.1f}s {'FAIL':<8} {msg}", flush=True)
            results.append((platform, url, "FAIL", msg, note))

    print(flush=True)
    print("=" * 130, flush=True)
    ok = sum(1 for r in results if r[2] == "OK")
    print(f"  probe success: {ok}/{len(results)}", flush=True)
    print(flush=True)

    print("OK sites (downloads will also work):", flush=True)
    for platform, url, status, detail, note in results:
        if status == "OK":
            print(f"  ✓ [{platform}] {url}", flush=True)
            print(f"      → {detail}", flush=True)

    print(flush=True)
    print("Expected failures (cookies-required sites):", flush=True)
    for platform, url, status, detail, note in results:
        if status != "OK" and note:
            print(f"  ⚠ [{platform}] {url}", flush=True)
            print(f"      → {detail}", flush=True)
            print(f"      note: {note}", flush=True)

    print(flush=True)
    print("Unexpected failures (zoinks bugs):", flush=True)
    unexpected = [r for r in results if r[2] != "OK" and not r[4]]
    if not unexpected:
        print("  (none — all failures were expected cookies-required sites)", flush=True)
    for platform, url, status, detail, note in unexpected:
        print(f"  ✗ [{platform}] {url}", flush=True)
        print(f"      → {detail}", flush=True)

    return 0 if not unexpected else 1

if __name__ == "__main__":
    sys.exit(main())
