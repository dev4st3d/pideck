#!/usr/bin/env python3
"""Pixel comparison only. Captures must come from the real native application.

Requires Pillow. No resizing, alignment, blurring, region exclusion or synthetic
rendering is applied. Original Midnight intentionally differs at the toolbar;
use --midnight-reference only for a separately approved corrected golden image.
"""
from __future__ import annotations
import argparse
import hashlib
import io
import json
import sys
import zipfile
from pathlib import Path
from PIL import Image, ImageChops, ImageEnhance, ImageStat

NAMES = (
    "01-original-completed.png",
    "02-original-ongoing-compact.png",
    "03-original-ongoing-expanded.png",
    "04-linen-light-ongoing-expanded.png",
    "05-graphite-dark-ongoing-expanded.png",
    "06-midnight-dark-ongoing-expanded.png",
)
REGIONS = {
    "titlebar": (0, 0, 1440, 40),
    "navigation_rail": (0, 40, 64, 960),
    "sidebar": (64, 40, 304, 960),
    "session_toolbar": (304, 40, 1440, 100),
    "conversation": (304, 100, 1440, 758),
    "queue_and_composer": (304, 758, 1440, 960),
}


def compare(expected: Image.Image, actual: Image.Image) -> tuple[dict, Image.Image]:
    if expected.size != actual.size:
        raise ValueError(f"Size mismatch: reference {expected.size}, capture {actual.size}; no scaling permitted")
    difference = ImageChops.difference(expected.convert("RGB"), actual.convert("RGB"))
    channels = difference.split()
    mask = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]), channels[2])
    equal_pixels = mask.histogram()[0]
    total = expected.width * expected.height
    stats = ImageStat.Stat(difference)
    return {
        "size": list(expected.size), "total_pixels": total,
        "different_pixels": total - equal_pixels,
        "exact_pixel_fraction": equal_pixels / total,
        "mean_absolute_rgb_error_0_255": sum(stats.mean) / 3,
        "max_channel_error_0_255": max(hi for _, hi in difference.getextrema()),
        "difference_bounds": difference.getbbox(),
        "pixel_exact": equal_pixels == total,
    }, difference


def load_image(data: bytes) -> Image.Image:
    with Image.open(io.BytesIO(data)) as image:
        image.load()
        if image.size != (1440, 960):
            raise ValueError(f"Expected a 1440x960 reference/capture, received {image.size}")
        return image.convert("RGB")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--design-zip", required=True, type=Path)
    parser.add_argument("--captures", required=True, type=Path)
    parser.add_argument("--output", default=Path("visual-artifacts/comparison"), type=Path)
    parser.add_argument("--midnight-reference", type=Path,
                        help="Optional approved Midnight golden with ONLY its toolbar corrected")
    args = parser.parse_args()
    missing = [name for name in NAMES if not (args.captures / name).is_file()]
    if missing:
        raise ValueError("Missing native captures: " + ", ".join(missing))
    if args.output.resolve() == args.captures.resolve():
        raise ValueError("Output must not overwrite the capture folder")
    args.output.mkdir(parents=True, exist_ok=True)
    report = {
        "scope": "Pixel comparison of supplied files, not proof of capture provenance.",
        "transforms_applied": [],
        "midnight_reference": "approved replacement" if args.midnight_reference else "original displaced-toolbar export",
        "screens": [],
    }
    with zipfile.ZipFile(args.design_zip) as archive:
        for name in NAMES:
            found = [entry for entry in archive.namelist() if entry.endswith("/images/" + name) or entry == "images/" + name]
            if len(found) != 1:
                raise ValueError(f"Expected one reference named {name}; found {len(found)}")
            reference_bytes = args.midnight_reference.read_bytes() if name == NAMES[-1] and args.midnight_reference else archive.read(found[0])
            capture_bytes = (args.captures / name).read_bytes()
            expected, actual = load_image(reference_bytes), load_image(capture_bytes)
            metrics, difference = compare(expected, actual)
            metrics.update({
                "name": name,
                "reference_sha256": hashlib.sha256(reference_bytes).hexdigest(),
                "capture_sha256": hashlib.sha256(capture_bytes).hexdigest(),
                "regions": {key: compare(expected.crop(box), actual.crop(box))[0] for key, box in REGIONS.items()},
            })
            report["screens"].append(metrics)
            stem = Path(name).stem
            difference.save(args.output / f"{stem}-difference.png")
            ImageEnhance.Brightness(difference).enhance(8).save(args.output / f"{stem}-difference-8x.png")
            Image.blend(expected, actual, 0.5).save(args.output / f"{stem}-overlay.png")
    report["all_pixel_exact"] = all(screen["pixel_exact"] for screen in report["screens"])
    output = args.output / "report.json"
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf8")
    print(output)
    return 0 if report["all_pixel_exact"] else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, zipfile.BadZipFile) as error:
        print(f"Comparison not completed: {error}", file=sys.stderr)
        raise SystemExit(2) from error
