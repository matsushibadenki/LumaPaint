#!/usr/bin/env python3
"""Compare registered lossless screen crops without changing their color space."""

import argparse
import hashlib
import io
import json
from pathlib import Path

from PIL import Image, ImageCms


def crop_spec(value):
    try:
        parts = tuple(int(part) for part in value.split(","))
    except ValueError as error:
        raise argparse.ArgumentTypeError("crop must be x,y,width,height") from error
    if len(parts) != 4 or min(parts[:2]) < 0 or min(parts[2:]) <= 0:
        raise argparse.ArgumentTypeError("crop must be nonnegative x,y and positive width,height")
    return parts


def load_crop(path, crop):
    # Pillow can silently decode 16-bit RGB PNG as 8-bit RGB. Reject it before
    # decoding so that a comparison never quietly loses precision.
    with open(path, "rb") as source:
        header = source.read(26)
    if header[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: only lossless PNG captures are accepted")
    if len(header) < 26 or header[12:16] != b"IHDR" or header[24] != 8:
        raise ValueError(f"{path}: expected an 8-bit PNG")
    with Image.open(path) as image:
        if image.format != "PNG":
            raise ValueError(f"{path}: only lossless PNG captures are accepted")
        if image.mode not in ("RGB", "RGBA"):
            raise ValueError(f"{path}: expected an 8-bit RGB or RGBA PNG, got {image.mode}")
        profile = image.info.get("icc_profile")
        if not profile:
            raise ValueError(f"{path}: embedded ICC profile is required")
        try:
            parsed_profile = ImageCms.ImageCmsProfile(io.BytesIO(profile))
        except (OSError, ImageCms.PyCMSError) as error:
            raise ValueError(f"{path}: invalid ICC profile") from error
        if parsed_profile.profile.xcolor_space.strip() != "RGB":
            raise ValueError(f"{path}: embedded ICC profile must describe RGB")
        x, y, width, height = crop
        if x + width > image.width or y + height > image.height:
            raise ValueError(f"{path}: crop extends beyond the image")
        region = image.crop((x, y, x + width, y + height))
        if region.mode == "RGBA":
            if region.getchannel("A").getextrema() != (255, 255):
                raise ValueError(f"{path}: screen crop must be opaque")
            region = region.convert("RGB")
        return region.tobytes(), profile


def compare(reference, candidate, reference_crop, candidate_crop):
    if reference_crop[2:] != candidate_crop[2:]:
        raise ValueError("crop dimensions must match; no scaling or registration is performed")
    ref, ref_profile = load_crop(reference, reference_crop)
    actual, actual_profile = load_crop(candidate, candidate_crop)
    if ref_profile != actual_profile:
        raise ValueError("embedded ICC profiles differ; convert both captures to the same space first")
    count = len(ref) // 3
    sums = [0, 0, 0]
    maxima = [0, 0, 0]
    differing_pixels = 0
    for i in range(0, len(ref), 3):
        differences = [abs(ref[i + c] - actual[i + c]) for c in range(3)]
        differing_pixels += any(differences)
        for c, difference in enumerate(differences):
            sums[c] += difference
            maxima[c] = max(maxima[c], difference)
    return {
        "reference": str(reference), "candidate": str(candidate),
        "reference_crop": reference_crop, "candidate_crop": candidate_crop,
        "icc_sha256": hashlib.sha256(ref_profile).hexdigest(),
        "pixels": count, "different_pixels": differing_pixels,
        "mean_absolute_difference_rgb_8bit": [total / count for total in sums],
        "max_absolute_difference_rgb_8bit": maxima,
        "note": "Encoded RGB differences; no ICC conversion, alignment, or resampling was applied.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--reference-crop", type=crop_spec, required=True, metavar="X,Y,W,H")
    parser.add_argument("--candidate-crop", type=crop_spec, required=True, metavar="X,Y,W,H")
    args = parser.parse_args()
    try:
        result = compare(args.reference, args.candidate, args.reference_crop, args.candidate_crop)
    except (OSError, ValueError) as error:
        parser.exit(2, f"error: {error}\n")
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
