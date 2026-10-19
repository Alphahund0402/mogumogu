"""Derives the three mood poses from the unchanged v2 sheet (pixel edit, no generator).

Munching: happy cell, mouth replaced by the neutral mouth, raised paws moved onto
an onigiri. Sleeping: neutral cell, eyes replaced by the vertically mirrored happy
eye arcs, three manga z marks. Worried: neutral cell, raised inner brows, wavy
mouth, sweat drop and two ticks. New strokes copy the measured ink: ~8-10 px
width, core rgb(60,62,70), darker rim, light halo, faint grain.

    py -3 design/brand/moods/derive.py assets/brand/mascots-v2.png assets/brand/mascots-v3-moods.png

Needs numpy, scipy, opencv-python and Pillow; deterministic (fixed seed).
Not part of the build.
"""
import sys
from pathlib import Path

import cv2
import numpy as np
from PIL import Image
from scipy import ndimage as ndi

SRC = Path(sys.argv[1])
OUT = Path(sys.argv[2])
CELL = 627
rng = np.random.default_rng(20261018)

INK = np.array([60.0, 62.0, 70.0])
INK_RIM = np.array([40.0, 42.0, 50.0])
HALO = np.array([251.0, 250.0, 248.0])
SKY = np.array([199.0, 224.0, 246.0])
RICE = np.array([253.0, 253.0, 250.0])
NORI = np.array([75.0, 85.0, 99.0])
SS = 4

base = np.asarray(Image.open(SRC).convert("RGB")).astype(float)
dark = base.mean(axis=2) < 150


def smooth_noise(n, sigma, amp):
    r = ndi.gaussian_filter1d(rng.standard_normal(max(n, 1200)), sigma, mode="wrap")
    return (r / (r.std() + 1e-9) * amp)[:n]


def resample(pts, step=0.35):
    pts = np.asarray(pts, float)
    seg = np.hypot(*np.diff(pts, axis=0).T)
    s = np.r_[0, np.cumsum(seg)]
    t = np.linspace(0, s[-1], max(2, int(s[-1] / step)))
    return np.c_[np.interp(t, s, pts[:, 0]), np.interp(t, s, pts[:, 1])]


def bezier(*ctrl, n=80):
    ctrl = np.asarray(ctrl, float)
    t = np.linspace(0, 1, n)[:, None]
    while len(ctrl) > 1:
        ctrl = np.array([(1 - t) * a + t * b for a, b in zip(ctrl[:-1], ctrl[1:])]) if ctrl.ndim == 3 else \
            np.array([(1 - t) * a + t * b for a, b in zip(ctrl[:-1], ctrl[1:])])
    return ctrl[0]


def arc(cx, cy, rx, ry, a0, a1, n=120, rot=0.0):
    a = np.radians(np.linspace(a0, a1, n))
    x, y = rx * np.cos(a), ry * np.sin(a)
    r = np.radians(rot)
    return np.c_[cx + x * np.cos(r) - y * np.sin(r), cy + x * np.sin(r) + y * np.cos(r)]


def box_of(pts, pad):
    h, w = base.shape[:2]
    x0 = int(max(0, np.floor(pts[:, 0].min() - pad)))
    y0 = int(max(0, np.floor(pts[:, 1].min() - pad)))
    x1 = int(min(w, np.ceil(pts[:, 0].max() + pad)))
    y1 = int(min(h, np.ceil(pts[:, 1].max() + pad)))
    return x0, y0, x1, y1


def stroke_alpha(pts, width, wobble=0.35, wvar=0.07, taper=0.12):
    """Round-capped hand stroke as anti-aliased alpha over the whole sheet."""
    pts = resample(pts)
    n = len(pts)
    d = np.gradient(pts, axis=0)
    nrm = np.c_[-d[:, 1], d[:, 0]]
    nrm /= np.linalg.norm(nrm, axis=1, keepdims=True) + 1e-9
    pts = pts + nrm * smooth_noise(n, 30, wobble)[:, None]
    widths = width * np.clip(1 + smooth_noise(n, 40, wvar), 0.75, 1.25)
    if taper:
        t = np.linspace(0, 1, n)
        widths *= 1 - taper * (np.clip(1 - t / 0.12, 0, 1) + np.clip((t - 0.88) / 0.12, 0, 1))
    x0, y0, x1, y1 = box_of(pts, width * 2)
    canvas = np.zeros(((y1 - y0) * SS, (x1 - x0) * SS), np.uint8)
    f = SS * 16
    for (x, y), w in zip(pts, widths):
        cv2.circle(canvas, (int(round((x - x0) * f)), int(round((y - y0) * f))), int(round(w / 2 * f)), 255, -1,
                   lineType=cv2.LINE_AA, shift=4)
    alpha = np.zeros(base.shape[:2])
    alpha[y0:y1, x0:x1] = cv2.resize(canvas, (x1 - x0, y1 - y0), interpolation=cv2.INTER_AREA) / 255.0
    return alpha


def shape_mask(polys, holes=()):
    """Supersampled binary mask of filled polygons minus holes, with its box."""
    pts = np.vstack(polys)
    x0, y0, x1, y1 = box_of(pts, 24)
    canvas = np.zeros(((y1 - y0) * SS, (x1 - x0) * SS), np.uint8)
    f = SS * 16

    def scaled(poly):
        return (np.asarray([[(x - x0) * f, (y - y0) * f] for x, y in poly])).round().astype(np.int32)

    cv2.fillPoly(canvas, [scaled(p) for p in polys], 255, lineType=cv2.LINE_8, shift=4)
    for p in holes:
        cv2.fillPoly(canvas, [scaled(p)], 0, lineType=cv2.LINE_8, shift=4)
    return canvas > 0, (x0, y0, x1, y1)


def downsample(mask_ss, box):
    x0, y0, x1, y1 = box
    alpha = np.zeros(base.shape[:2])
    alpha[y0:y1, x0:x1] = cv2.resize(mask_ss.astype(np.float32), (x1 - x0, y1 - y0), interpolation=cv2.INTER_AREA)
    return alpha


def outline_alpha(mask_ss, box, width, wvar=0.07):
    """Contour of a filled shape, centred on its edge, slightly uneven like the pencil."""
    d_in = ndi.distance_transform_edt(mask_ss)
    d_out = ndi.distance_transform_edt(~mask_ss)
    sd = np.where(mask_ss, d_in, d_out)
    field = ndi.gaussian_filter(rng.standard_normal(mask_ss.shape), 18 * SS)
    field /= field.std() + 1e-9
    half = width / 2 * SS * (1 + wvar * field)
    return downsample(sd < half, box)


def ink(img, alpha, color=INK):
    solid = alpha > 0.5
    d_out = ndi.distance_transform_edt(~solid)
    halo = np.clip(1 - d_out / 2.6, 0, 1) * (~solid) * 0.5
    img = img * (1 - halo[..., None]) + HALO * halo[..., None]
    d_in = ndi.distance_transform_edt(solid)
    rim = np.clip(1 - (d_in - 0.6) / 1.6, 0, 1)
    grain = ndi.gaussian_filter(rng.standard_normal(alpha.shape), 0.8)
    grain = grain / (grain.std() + 1e-9) * 6
    col = color + grain[..., None] - rim[..., None] * (color - INK_RIM)
    return img * (1 - alpha[..., None]) + col * alpha[..., None]


def fill(img, alpha, color, grain=1.6, streak=0.0):
    g = ndi.gaussian_filter(rng.standard_normal(alpha.shape), (0.6, 2.4 if streak else 0.6))
    g = g / (g.std() + 1e-9) * grain
    return img * (1 - alpha[..., None]) + (color + g[..., None]) * alpha[..., None]


def feather(mask, sigma=1.6):
    return np.clip(ndi.gaussian_filter(mask.astype(float), sigma) * 1.6, 0, 1)


def patch(img, box, dx, dy, sigma=1.8, shape="ellipse"):
    """Clone-stamp: covers box with fur taken from the same image shifted by dx, dy."""
    x0, y0, x1, y1 = box
    h, w = img.shape[:2]
    m = np.zeros((h, w))
    if shape == "ellipse":
        yy, xx = np.ogrid[:h, :w]
        cx, cy, rx, ry = (x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2, (y1 - y0) / 2
        m[((xx - cx) / rx) ** 2 + ((yy - cy) / ry) ** 2 <= 1] = 1
    else:
        m[y0:y1, x0:x1] = 1
    m = feather(m, sigma)
    src = np.roll(img, (-dy, -dx), axis=(0, 1))
    return img * (1 - m[..., None]) + src * m[..., None]


def transplant(img, src_box, centre, flip_v=False, flip_h=False, grow=3):
    """Moves an existing ink mark (with its rim and halo) to a new centre."""
    x0, y0, x1, y1 = src_box
    piece = base[y0:y1, x0:x1].copy()
    m = ndi.binary_dilation(dark[y0:y1, x0:x1], iterations=grow).astype(float)
    if flip_v:
        piece, m = piece[::-1], m[::-1]
    if flip_h:
        piece, m = piece[:, ::-1], m[:, ::-1]
    m = feather(m > 0, 1.0)
    h, w = m.shape
    tx, ty = int(round(centre[0] - w / 2)), int(round(centre[1] - h / 2))
    region = img[ty:ty + h, tx:tx + w]
    img[ty:ty + h, tx:tx + w] = region * (1 - m[..., None]) + piece * m[..., None]
    return img


def restore(img, source, alpha):
    return img * (1 - alpha[..., None]) + source * alpha[..., None]


def rounded_triangle(top, left, right, radius, n=48):
    """Convex hull of three corner circles: a soft onigiri silhouette."""
    pts = np.array([top, left, right], float)
    centre = pts.mean(axis=0)
    circles = []
    for p in pts:
        v = centre - p
        c = p + v / np.linalg.norm(v) * radius * 1.9
        a = np.linspace(0, 2 * np.pi, n, endpoint=False)
        circles.append(np.c_[c[0] + radius * np.cos(a), c[1] + radius * np.sin(a)])
    allp = np.vstack(circles).astype(np.float32)
    hull = cv2.convexHull(allp).reshape(-1, 2)
    return hull.astype(float)


# ---------------------------------------------------------------- munching
def munching():
    img = base.copy()
    # Open happy mouth -> fur from the chest below, then the neutral mouth.
    img = patch(img, (902, 264, 971, 332), 0, 72)
    img = transplant(img, (304, 288, 370, 322), (936.5, 289))
    # Raised paws -> fur from the chest beside them.
    img = patch(img, (782, 326, 872, 404), 84, 0, shape="rect", sigma=2.4)
    img = patch(img, (988, 346, 1078, 420), -86, 0, shape="rect", sigma=2.4)
    fur = img.copy()

    # Onigiri held at the chest, one bite taken from the upper right.
    top, left, right = (931, 314), (836, 460), (1026, 460)
    body = rounded_triangle(top, left, right, 25)
    bite = [arc(963, 350, 13, 13, 0, 360, 48), arc(977, 371, 12, 12, 0, 360, 48)]
    mask, box = shape_mask([body], holes=bite)
    img = fill(img, downsample(mask, box), RICE, grain=1.2)
    # Nori band with slightly uneven sides, clipped to the rice.
    nori = np.vstack([bezier([903, 404], [916, 401], [945, 401], [959, 404], n=30),
                      bezier([959, 404], [961, 430], [962, 450], [964, 480], n=30)[1:],
                      bezier([964, 480], [940, 482], [922, 482], [898, 480], n=30)[1:],
                      bezier([898, 480], [900, 450], [901, 430], [903, 404], n=30)[1:]])
    canvas = np.zeros(mask.shape, np.uint8)
    f = SS * 16
    cv2.fillPoly(canvas, [((nori - [box[0], box[1]]) * f).round().astype(np.int32)], 255, lineType=cv2.LINE_8, shift=4)
    band_ss = (canvas > 0) & mask
    img = fill(img, downsample(band_ss, box), NORI, grain=5.0, streak=1)
    # A few faint rice grains as fill texture.
    for p in ([[872, 430], [879, 426]], [[928, 352], [935, 350]], [[992, 432], [998, 436]], [[905, 378], [911, 381]]):
        a = stroke_alpha(p, 3.0, wobble=0.1) * 0.2
        img = img * (1 - a[..., None]) + np.array([150.0, 152, 158]) * a[..., None]
    img = ink(img, outline_alpha(mask, box, 10.0))
    ss_edge = band_ss & ~ndi.binary_erosion(band_ss, iterations=SS * 2) & ndi.binary_erosion(mask, iterations=SS * 3)
    img = ink(img, downsample(ss_edge, box) * 0.9)

    # Raised paws resting on the rice: fur restored inside, contour over the top.
    for cx, cy, a0, a1, rot in ((870, 396, 150, 395, -6), (992, 396, 145, 390, 6)):
        yy, xx = np.ogrid[:img.shape[0], :img.shape[1]]
        r = np.radians(rot)
        u = (xx - cx) * np.cos(r) + (yy - cy) * np.sin(r)
        v = -(xx - cx) * np.sin(r) + (yy - cy) * np.cos(r)
        inside = (u / 29.0) ** 2 + (v / 25.0) ** 2 <= 1
        img = restore(img, fur, feather(inside, 0.9))
        img = ink(img, stroke_alpha(arc(cx, cy, 30, 26, a0, a1, rot=rot), 9.5, taper=0.25))
    # Two rice crumbs beside the chewing mouth.
    for cx, cy, rot in ((890, 304, -25), (984, 298, 30)):
        cm, cb = shape_mask([arc(cx, cy, 6.0, 4.0, 0, 360, 40, rot=rot)])
        img = fill(img, downsample(cm, cb), RICE, grain=0.6)
        img = ink(img, outline_alpha(cm, cb, 4.0, wvar=0.02))
    return img[0:CELL, CELL:2 * CELL]


# ---------------------------------------------------------------- sleeping
def sleeping():
    img = base.copy()
    img = patch(img, (226, 262, 274, 311), 0, -52)
    img = patch(img, (398, 251, 446, 299), 0, -52)
    # Mirrored happy eye arcs: the same pen, closed downwards.
    img = transplant(img, (831, 240, 888, 269), (250, 291), flip_v=True)
    img = transplant(img, (995, 256, 1052, 289), (422, 280), flip_v=True)

    def zed(cx, cy, s, w):
        h = s * 0.92
        pts = [[cx - s / 2, cy - h / 2], [cx + s / 2, cy - h / 2 + 1.5], [cx - s / 2 + 1.0, cy + h / 2 - 0.5],
               [cx + s / 2 + 1.5, cy + h / 2]]
        corner = []
        for a, b in zip(pts[:-1], pts[1:]):
            corner.extend(resample([a, b], 0.5).tolist())
        return stroke_alpha(corner, w, wobble=0.3, taper=0.2)

    img = ink(img, zed(518, 128, 34, 9.5))
    img = ink(img, zed(556, 84, 25, 8.5))
    img = ink(img, zed(586, 50, 18, 7.5))
    return img[0:CELL, 0:CELL]


# ---------------------------------------------------------------- worried
def worried():
    img = base.copy()
    # Mouth -> chin fur, then a small wavy line.
    img = patch(img, (302, 286, 371, 324), 0, 44)
    wave = np.vstack([bezier([313, 311], [322, 300], [330, 300], [337, 308]),
                      bezier([337, 308], [344, 316], [352, 316], [360, 305])[1:]])
    img = ink(img, stroke_alpha(wave, 8.5, taper=0.2))
    # Inner ends of the brows raised.
    img = ink(img, stroke_alpha(bezier([229, 252], [244, 249], [256, 244], [267, 234]), 8.5, taper=0.3))
    img = ink(img, stroke_alpha(bezier([403, 223], [414, 233], [427, 238], [443, 241]), 8.5, taper=0.3))
    # Sweat drop beside the head, tip up.
    tip = np.array([510.0, 153.0])
    drop = np.vstack([bezier(tip, [503, 172], [491, 184], [491, 197]),
                      arc(509, 197, 18, 18, 180, 0, 60)[1:],
                      bezier([527, 197], [527, 184], [517, 172], tip)[1:]])
    dm, db = shape_mask([drop])
    img = fill(img, downsample(dm, db), SKY, grain=2.0)
    hm, hb = shape_mask([arc(502, 198, 4.6, 6.4, 0, 360, 40, rot=20)])
    img = fill(img, downsample(hm, hb), np.array([252.0, 253, 255]), grain=0.5)
    img = ink(img, outline_alpha(dm, db, 7.5, wvar=0.04))
    # Two worry ticks, same weight as the existing ones.
    img = ink(img, stroke_alpha([[550, 104], [562, 88]], 10.0, taper=0.15))
    img = ink(img, stroke_alpha([[568, 132], [587, 124]], 10.0, taper=0.15))
    return img[0:CELL, 0:CELL]


blank = np.zeros((CELL, CELL, 3)) + base[20:80, 20:80].reshape(-1, 3).mean(axis=0)
blank += ndi.gaussian_filter(rng.standard_normal(blank.shape[:2]), 0.7)[..., None] * 0.9

sheet = np.zeros((2 * CELL, 2 * CELL, 3))
sheet[0:CELL, 0:CELL] = munching()
sheet[0:CELL, CELL:] = sleeping()
sheet[CELL:, 0:CELL] = worried()
sheet[CELL:, CELL:] = blank
Image.fromarray(np.clip(sheet, 0, 255).round().astype(np.uint8)).save(OUT, optimize=True)
print("written", OUT)
