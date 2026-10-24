#!/usr/bin/env python3
"""Read-only contour measurement of native Slint PPM output; Python standard library.

Artwork is never modified. FWHM is measured across a local contour normal, in
logical pixels. Spatial quartiles describe hand-drawn variation, not uncertainty.
"""
from pathlib import Path
import argparse
import csv
import hashlib
import html
import json
import math
import statistics
import struct
import zlib

ROOT = Path(__file__).resolve().parents[3]
POSES = [("neutral", "Neutral", 7, (320, 275)),
         ("happy", "Fröhlich", 0, (315, 275)),
         ("folder", "Mit Ordner", 1, (320, 270)),
         ("focused", "Am Laptop", 2, (315, 285))]
# Matching unobstructed head/ear regions, selected before measuring widths.
# Whiskers, expressions, eyes, cheek hatching and props are excluded.
ARCS = {"left_cheek": (-166, -150), "left_ear": (-145, -135),
        "forehead": (-104, -90), "right_ear": (-64, -54),
        "right_cheek": (-38, -23)}
PAGES = {0: "happy", 1: "folder", 2: "focused", 3: "focused",
         4: "folder", 5: "focused", 6: "neutral", 7: "neutral"}


class Image:
    def __init__(self, path):
        content = path.read_bytes()
        magic, dimensions, depth, self.pixels = content.split(b"\n", 3)
        assert magic == b"P6" and depth == b"255", path
        self.width, self.height = map(int, dimensions.split())
        assert len(self.pixels) == self.width * self.height * 3, path
        assert self.width == self.height, path
        self.scale = self.width / 104

    def luma(self, x, y):
        # Sample in logical coordinates, at physical pixel centers.
        x = x * self.scale - 0.5
        y = y * self.scale - 0.5
        if not (0 <= x < self.width - 1 and 0 <= y < self.height - 1):
            return 253.0
        ix, iy = math.floor(x), math.floor(y)
        fx, fy = x - ix, y - iy
        result = 0.0
        for dx, dy, weight in [(0, 0, (1-fx)*(1-fy)), (1, 0, fx*(1-fy)),
                               (0, 1, (1-fx)*fy), (1, 1, fx*fy)]:
            offset = ((iy+dy)*self.width+ix+dx)*3
            r, g, b = self.pixels[offset:offset+3]
            result += weight * (0.2126*r + 0.7152*g + 0.0722*b)
        return result


def add(a, b, factor=1):
    return (a[0] + b[0]*factor, a[1] + b[1]*factor)


def boundary(image, center, angle):
    direction = (math.cos(math.radians(angle)), math.sin(math.radians(angle)))
    previous = 66.0
    for step in range(1, 1151):
        radius = 66.0 - step * 0.04
        point = add(center, direction, radius)
        value = image.luma(*point)
        if value < 155:
            # Refine the first outer-to-inner threshold crossing.
            lo, hi = radius, previous
            for _ in range(10):
                mid = (lo+hi)/2
                if image.luma(*add(center, direction, mid)) < 155:
                    lo = mid
                else:
                    hi = mid
            return add(center, direction, (lo+hi)/2)
        previous = radius
    raise ValueError(f"No boundary at {angle} degrees")


def landmarks(image, source_center):
    # source-clip: cell=627, inset=25, square=577, native viewport=104.
    center = tuple((p-25)*104/577 for p in source_center)
    points, missing = [], []
    for region, (start, end) in ARCS.items():
        for index in range(9):
            angle = start + (end-start)*index/8
            try:
                point = boundary(image, center, angle)
                before = boundary(image, center, angle-2)
                after = boundary(image, center, angle+2)
            except ValueError as error:
                missing.append({"region": region, "angle": angle, "reason": str(error)})
                continue
            tangent = (after[0]-before[0], after[1]-before[1])
            length = math.hypot(*tangent)
            normal = (-tangent[1]/length, tangent[0]/length)
            points.append({"region": region, "angle": angle, "point": point, "normal": normal})
    return points, missing


def width(image, landmark):
    point, normal = landmark["point"], landmark["normal"]
    step = 0.025
    offsets = [-4 + i*step for i in range(321)]
    values = [image.luma(*add(point, normal, d)) for d in offsets]
    # Use the local dark peak only; do not collect a nearby parallel stroke.
    candidates = [i for i, d in enumerate(offsets) if abs(d) <= 1.3]
    peak = min(candidates, key=values.__getitem__)
    outer = statistics.median(values[:21])
    inner = statistics.median(values[-21:])
    baseline = (outer+inner)/2
    threshold = (baseline+values[peak])/2
    if baseline-values[peak] < 30:
        raise ValueError("Insufficient contour contrast")
    left = peak
    right = peak
    while left > 0 and values[left-1] < threshold:
        left -= 1
    while right < len(values)-1 and values[right+1] < threshold:
        right += 1
    if left == 0 or right == len(values)-1:
        raise ValueError("Contour extends beyond profile")
    a = offsets[left-1] + step*(values[left-1]-threshold)/(values[left-1]-values[left])
    b = offsets[right] + step*(threshold-values[right])/(values[right+1]-values[right])
    result = b-a
    if not 0.3 < result < 3.5:
        raise ValueError(f"Unexpected contour width: {result}")
    return {"width": result, "minimum_luma": values[peak], "baseline_luma": baseline}


def quantiles(values):
    q1, _, q3 = statistics.quantiles(values, n=4, method="inclusive")
    return {"median": statistics.median(values), "q1": q1, "q3": q3,
            "min": min(values), "max": max(values), "samples": len(values)}


def png(path, image):
    """Lossless encoding of renderer pixels, for evidence only."""
    def chunk(kind, data):
        return struct.pack(">I", len(data))+kind+data+struct.pack(">I", zlib.crc32(kind+data))
    rows = b"".join(b"\0"+image.pixels[y*image.width*3:(y+1)*image.width*3]
                    for y in range(image.height))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB",
        image.width, image.height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def profile_map(output, rows, summaries):
    """An annotation map, not edited artwork: native pixels plus separate SVG lines."""
    cards = []
    for name, label, _, _ in POSES:
        samples = [r for r in rows if r["pose"] == name and r["dpi"] == 400]
        median = next(s["median"] for s in summaries if s["pose"] == name and s["dpi"] == 400)
        lines = []
        for row in samples:
            x, y, nx, ny = (row[k] for k in ["x", "y", "nx", "ny"])
            lines.append(f'<line x1="{4*(x-2*nx):.2f}" y1="{4*(y-2*ny):.2f}" '
                         f'x2="{4*(x+2*nx):.2f}" y2="{4*(y+2*ny):.2f}">'
                         f'<title>{html.escape(row["region"])}: {row["width"]:.2f} px</title></line>')
        cards.append(f'<article><h2>{label} · {median:.2f} px</h2>'
                     f'<p>100 % · Originalgröße 104 × 104 px</p>'
                     f'<img src="{name}-100.png" width="104" height="104" alt="{label}, native Größe">'
                     f'<p>400 % · dieselbe native Bildkomponente</p>'
                     f'<svg viewBox="0 0 416 416" role="img" aria-label="{label}: Kontur und Messstellen">'
                     f'<image href="{name}-400.png" width="416" height="416"/>'
                     f'<g class="profiles" stroke="#b12954" stroke-width="1.25">{"".join(lines)}</g>'
                     '</svg></article>')
    (output / "profiles.html").write_text('''<!doctype html>
<html lang="de"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Mochi · Messstellen der Konturen</title>
<style>
*{box-sizing:border-box}body{margin:24px auto;padding:0 20px;max-width:1000px;background:#fffdf8;color:#243951;font:16px/1.5 system-ui}
h1{font-size:25px}h2{font-size:18px}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:20px;margin-top:20px}
article{padding:16px;border:1px solid #d3deea;border-radius:10px;background:white}svg{width:100%;max-width:416px}
article p{font-size:13px;color:#536477}#lines:not(:checked)~main .profiles{display:none}a{color:#1766a4}
</style>
<h1>Mochi: native Konturen im Vergleich</h1>
<p>43 vergleichbare Messstellen je Pose, quer zu Kopf- und Ohrkonturen.
Die Zahlen sind Mediane der Halbwertsbreite (FWHM) bei 400 %, auf logische Pixel zurückgerechnet.
Die Bilder stammen aus der kompilierten Slint-Anwendung; sie wurden für diesen Bericht nicht nachgezeichnet.</p>
<p><a href="README.md">Messbericht und Grenzen</a> · <a href="samples.csv">Rohmessungen (CSV)</a></p>
<input id="lines" type="checkbox" checked><label for="lines"> Messlinien anzeigen (Wert per Mauszeiger)</label>
<main>''' + "".join(cards) + '</main></html>\n', encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=ROOT / "target/stroke-qa")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((args.input / "source-manifest.json").read_text(encoding="utf-8-sig"))
    for name, expected in manifest["sources"].items():
        assert hashlib.sha256((ROOT/name).read_bytes()).hexdigest() == expected, f"Rerender changed source: {name}"
    for name, expected in manifest["frames"].items():
        assert hashlib.sha256((args.input/name).read_bytes()).hexdigest() == expected, f"Changed native frame: {name}"
    # Identity check covers every page and both supported window sizes at all DPIs.
    identical = 0
    for dpi in [100, 125, 150, 200, 400]:
        for _, _, page, _ in POSES:
            expected = Image(args.input / f"900x560-{dpi}-{page}.ppm").pixels
            for p, name in PAGES.items():
                if name != PAGES[page]:
                    continue
                for size in ["900x560", "1080x680"]:
                    actual = Image(args.input / f"{size}-{dpi}-{p}.ppm").pixels
                    assert actual == expected, (size, dpi, p)
                    identical += 1
    rows, summaries, rejected = [], [], []
    references = {}
    for name, _, page, center in POSES:
        reference = Image(args.input / f"900x560-400-{page}.ppm")
        points, missing = landmarks(reference, center)
        rejected.extend({"pose": name, "dpi": "reference landmark", **value} for value in missing)
        references[name] = points
    # Use only corresponding angles available in all four poses (no weighting bias).
    shared = set.intersection(*({(p["region"], p["angle"]) for p in points}
                                for points in references.values()))
    for name, label, page, _ in POSES:
        points = [p for p in references[name] if (p["region"], p["angle"]) in shared]
        for dpi in [100, 125, 150, 200, 400]:
            image = Image(args.input / f"900x560-{dpi}-{page}.ppm")
            values = []
            for point in points:
                try:
                    measured = width(image, point)
                except ValueError as error:
                    rejected.append({"pose": name, "dpi": dpi, **point, "reason": str(error)})
                    continue
                values.append(measured["width"])
                rows.append({"pose": name, "dpi": dpi, "region": point["region"],
                             "angle": point["angle"], "x": point["point"][0], "y": point["point"][1],
                             "nx": point["normal"][0], "ny": point["normal"][1], **measured})
            summaries.append({"pose": name, "label": label, "dpi": dpi, **quantiles(values)})
            if dpi in [100, 400]:
                png(args.output / f"{name}-{dpi}.png", image)
    calibration = []
    for dpi in [100, 125, 150, 200, 400]:
        image = Image(args.input / f"calibration-{dpi}.ppm")
        for x, expected in [(20, 0.75), (40, 1), (60, 1.5), (80, 2)]:
            measured = width(image, {"point": (x+expected/2, 52), "normal": (1, 0)})["width"]
            calibration.append({"dpi": dpi, "expected": expected, "measured": measured,
                                "error": measured-expected})
    assert max(abs(v["error"]) for v in calibration if v["dpi"] == 400) < 0.02, calibration
    with (args.output / "samples.csv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows({k: f"{v:.6f}" if isinstance(v, float) else v for k, v in row.items()}
                         for row in rows)
    sources = dict(manifest["sources"])
    sources["design/brand/measurement/measure.py"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    result = {"source_sha256": sources,
              "native_frame_sha256": manifest["frames"],
              "generated_ui_sha256": manifest["generated_ui_sha256"],
              "metric": "local-normal weighted-sRGB-brightness FWHM in logical pixels",
              "identity_checks": identical, "shared_landmarks_per_pose": len(shared),
              "summary": summaries, "calibration": calibration, "rejected": rejected}
    (args.output / "results.json").write_text(json.dumps(result, indent=2, ensure_ascii=False)+"\n", encoding="utf-8")
    profile_map(args.output, rows, summaries)
    for value in summaries:
        print(f'{value["pose"]:8} {value["dpi"]:3}%: median={value["median"]:.3f} '
              f'IQR={value["q1"]:.3f}..{value["q3"]:.3f} n={value["samples"]}')
    print(f"Identity checks: {identical}; rejected profiles: {len(rejected)}")
    print("Calibration at 400%:", [(v["expected"], round(v["measured"], 4))
                                   for v in calibration if v["dpi"] == 400])


if __name__ == "__main__":
    main()
