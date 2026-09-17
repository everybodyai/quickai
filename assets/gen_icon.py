"""Regenerates icon.png, icon.ico, and icons/icon-*.png from scratch.

Run from anywhere: `python3 assets/gen_icon.py` (needs Pillow + numpy).
"""
import os

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter

SIZE = 512
SS = 4  # supersample factor, downsampled at the end for clean anti-aliasing
C = SIZE * SS


def S(v):
    """Scale a 512-space coordinate/length into supersampled canvas space."""
    return v * SS


def lerp_rgba(size, t, color_a, color_b):
    """t: HxW float32 array in [0,1]. Returns an RGBA image."""
    h, w = t.shape
    out = np.empty((h, w, 4), dtype=np.uint8)
    for i in range(4):
        out[..., i] = np.clip(color_a[i] + (color_b[i] - color_a[i]) * t, 0, 255).astype(np.uint8)
    return Image.fromarray(out, "RGBA")


def linear_t(size, p0, p1):
    w, h = size
    ys, xs = np.mgrid[0:h, 0:w].astype(np.float32)
    dx, dy = p1[0] - p0[0], p1[1] - p0[1]
    length2 = dx * dx + dy * dy
    t = ((xs - p0[0]) * dx + (ys - p0[1]) * dy) / length2
    return np.clip(t, 0, 1)


def radial_t(size, center, radius):
    w, h = size
    ys, xs = np.mgrid[0:h, 0:w].astype(np.float32)
    dist = np.sqrt((xs - center[0]) ** 2 + (ys - center[1]) ** 2)
    return np.clip(dist / radius, 0, 1)


def linear_gradient(size, p0, p1, color_a, color_b):
    return lerp_rgba(size, linear_t(size, p0, p1), color_a, color_b)


def radial_gradient(size, center, radius, color_a, color_b):
    return lerp_rgba(size, radial_t(size, center, radius), color_a, color_b)


def mask_from_t(t):
    return Image.fromarray((np.clip(t, 0, 1) * 255).astype(np.uint8), "L")


def mask_from_draw(size, draw_fn):
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    draw_fn(d)
    return m


def paste_gradient(canvas, gradient_img, mask):
    canvas.paste(gradient_img, (0, 0), mask)


def tapered_ribbon(points, widths):
    pts = np.array(points, dtype=np.float64)
    n = len(pts)
    lefts, rights = [], []
    for i in range(n):
        if i == 0:
            d = pts[1] - pts[0]
        elif i == n - 1:
            d = pts[i] - pts[i - 1]
        else:
            d = pts[i + 1] - pts[i - 1]
        norm = np.linalg.norm(d)
        d = d / norm if norm > 1e-9 else np.array([1.0, 0.0])
        perp = np.array([-d[1], d[0]])
        w = widths[i] / 2
        lefts.append(pts[i] + perp * w)
        rights.append(pts[i] - perp * w)
    return [tuple(p) for p in lefts] + [tuple(p) for p in reversed(rights)]


size = (C, C)
# Fully opaque square canvas at supersampled res -- rounded corners are cut
# in as an alpha mask *after* the final downsample, so the transparent-to-
# opaque edge never goes through a Lanczos resize (which otherwise haloes).
img = Image.new("RGBA", size, (0, 0, 0, 255))
cx = C // 2

# a plain rounded-rect mask, reused below to clip a couple of effects to the
# icon's silhouette (not for cutting transparency yet)
bg_mask = mask_from_draw(
    size,
    lambda d: d.rounded_rectangle([S(8), S(8), C - S(8), C - S(8)], radius=S(100), fill=255),
)

# ---------------------------------------------------------------- background
bg_grad = radial_gradient(
    size, (S(150), S(90)), S(520), (206, 232, 248, 255), (146, 196, 228, 255)
)
img.paste(bg_grad, (0, 0))

# ground: a light-brown band the pot sits on, blended into the sky over a
# soft horizon band rather than a hard line
horizon_y = 400
ground_t = linear_t(size, (0, S(horizon_y - 16)), (0, S(horizon_y + 16)))
ground_mask = mask_from_t(ground_t)
ground_grad = linear_gradient(
    size, (0, S(horizon_y)), (0, C), (222, 195, 154, 255), (182, 148, 104, 255)
)
img.paste(ground_grad, (0, 0), ground_mask)

# subtle top-edge sheen + bottom-edge grounding shadow, instead of a full
# ring bevel (which read as a heavy dark frame against the light sky/ground)
top_sheen_mask = mask_from_draw(
    size,
    lambda d: d.rounded_rectangle(
        [S(8), S(8), C - S(8), C - S(8)], radius=S(100), outline=255, width=S(10)
    ),
)
top_sheen_mask = Image.composite(top_sheen_mask, Image.new("L", size, 0), bg_mask)
top_sheen_mask = ImageChops.multiply(
    top_sheen_mask, mask_from_t(1 - linear_t(size, (0, S(60)), (0, S(220))))
)
top_sheen_mask = top_sheen_mask.filter(ImageFilter.GaussianBlur(S(5)))
img.paste(Image.new("RGBA", size, (255, 255, 255, 70)), (0, 0), top_sheen_mask)

bottom_shade_mask = mask_from_draw(
    size,
    lambda d: d.rounded_rectangle(
        [S(8), S(8), C - S(8), C - S(8)], radius=S(100), outline=255, width=S(10)
    ),
)
bottom_shade_mask = Image.composite(bottom_shade_mask, Image.new("L", size, 0), bg_mask)
bottom_shade_mask = ImageChops.multiply(
    bottom_shade_mask, mask_from_t(linear_t(size, (0, S(300)), (0, S(460))))
)
bottom_shade_mask = bottom_shade_mask.filter(ImageFilter.GaussianBlur(S(5)))
img.paste(Image.new("RGBA", size, (60, 40, 20, 60)), (0, 0), bottom_shade_mask)

# ---------------------------------------------------------------- geometry
pot_top_y, pot_bot_y = 358, 430
pot_top_half, pot_bot_half = 72, 97
rim_y0, rim_y1 = 344, 366

# ground-contact shadow, blurred, sits behind the pot
shadow = Image.new("RGBA", size, (0, 0, 0, 0))
sd = ImageDraw.Draw(shadow)
sd.ellipse(
    [S(cx / SS - pot_bot_half - 14), S(pot_bot_y - 14), S(cx / SS + pot_bot_half + 14), S(pot_bot_y + 34)],
    fill=(0, 0, 0, 130),
)
shadow = shadow.filter(ImageFilter.GaussianBlur(S(10)))
img.alpha_composite(shadow)

# ---------------------------------------------------------------- pot body
pot_mask = mask_from_draw(
    size,
    lambda d: d.polygon(
        [
            (cx - S(pot_top_half), S(pot_top_y)),
            (cx + S(pot_top_half), S(pot_top_y)),
            (cx + S(pot_bot_half), S(pot_bot_y)),
            (cx - S(pot_bot_half), S(pot_bot_y)),
        ],
        fill=255,
    ),
)
pot_grad = linear_gradient(
    size,
    (cx - S(pot_bot_half), S(pot_top_y)),
    (cx + S(pot_bot_half), S(pot_top_y)),
    (230, 158, 100, 255),
    (146, 82, 46, 255),
)
paste_gradient(img, pot_grad, pot_mask)

# vertical shading pass for a rounder, less flat pot: darken the bottom edge
pot_shade = linear_gradient(
    size, (0, S(pot_top_y)), (0, S(pot_bot_y)), (0, 0, 0, 0), (0, 0, 0, 60)
)
pot_shade_layer = Image.new("RGBA", size, (0, 0, 0, 0))
pot_shade_layer.paste(pot_shade, (0, 0), pot_mask)
img.alpha_composite(pot_shade_layer)

# a thin highlight seam near the pot's left edge (specular catch-light)
hl_mask = mask_from_draw(
    size,
    lambda d: d.polygon(
        [
            (cx - S(pot_top_half) + S(6), S(pot_top_y) + S(6)),
            (cx - S(pot_top_half) + S(16), S(pot_top_y) + S(4)),
            (cx - S(pot_bot_half) + S(22), S(pot_bot_y) - S(6)),
            (cx - S(pot_bot_half) + S(10), S(pot_bot_y) - S(6)),
        ],
        fill=255,
    ),
)
hl_mask = hl_mask.filter(ImageFilter.GaussianBlur(S(3)))
hl_layer = Image.new("RGBA", size, (255, 224, 190, 90))
img.paste(hl_layer, (0, 0), Image.composite(hl_mask, Image.new("L", size, 0), pot_mask))

# ---------------------------------------------------------------- pot rim
rim_mask = mask_from_draw(
    size,
    lambda d: d.rounded_rectangle(
        [cx - S(pot_top_half + 10), S(rim_y0), cx + S(pot_top_half + 10), S(rim_y1)],
        radius=S(8),
        fill=255,
    ),
)
rim_grad = linear_gradient(
    size,
    (cx - S(pot_top_half), S(rim_y0)),
    (cx + S(pot_top_half), S(rim_y0)),
    (244, 178, 122, 255),
    (168, 100, 60, 255),
)
paste_gradient(img, rim_grad, rim_mask)
# crease shadow where the rim meets the body
crease = mask_from_draw(size, lambda d: d.line(
    [(cx - S(pot_top_half + 10), S(rim_y1)), (cx + S(pot_top_half + 10), S(rim_y1))],
    fill=255, width=S(3),
))
crease = crease.filter(ImageFilter.GaussianBlur(S(2)))
img.paste(Image.new("RGBA", size, (0, 0, 0, 90)), (0, 0), crease)
# top highlight line of the rim's front lip
rim_top_hl = mask_from_draw(size, lambda d: d.line(
    [(cx - S(pot_top_half + 6), S(rim_y0) + S(2)), (cx + S(pot_top_half + 6), S(rim_y0) + S(2))],
    fill=255, width=S(2),
))
rim_top_hl = rim_top_hl.filter(ImageFilter.GaussianBlur(S(1.5)))
img.paste(Image.new("RGBA", size, (255, 230, 200, 110)), (0, 0), rim_top_hl)

# ---------------------------------------------------------------- soil
# nested fully inside the rim band (rim_y0..rim_y1), so it can't itself
# poke out onto the background above or the pot body below.
soil_cx, soil_cy = cx, S(rim_y0) + S(11)
soil_mask = mask_from_draw(
    size,
    lambda d: d.ellipse(
        [soil_cx - S(pot_top_half - 6), soil_cy - S(10), soil_cx + S(pot_top_half - 6), soil_cy + S(10)],
        fill=255,
    ),
)
soil_grad = radial_gradient(
    size, (soil_cx - S(14), soil_cy - S(4)), S(80), (74, 52, 36, 255), (34, 22, 15, 255)
)
paste_gradient(img, soil_grad, soil_mask)

# ---------------------------------------------------------------- trunk
main_pts = [(cx - S(2), soil_cy - S(2)), (cx + S(30), S(300)), (cx - S(4), S(228))]
main_widths = [S(46), S(30), S(20)]
branch1_pts = [(cx - S(4), S(228)), (cx - S(58), S(186))]
branch1_widths = [S(20), S(10)]
branch2_pts = [(cx - S(4), S(228)), (cx + S(36), S(180))]
branch2_widths = [S(17), S(8)]

trunk_mask_img = Image.new("L", size, 0)
td = ImageDraw.Draw(trunk_mask_img)
for pts, widths in ((main_pts, main_widths), (branch1_pts, branch1_widths), (branch2_pts, branch2_widths)):
    td.polygon(tapered_ribbon(pts, widths), fill=255)
    # round the joints/tips so segments blend smoothly
    td.ellipse(
        [pts[0][0] - widths[0] / 2, pts[0][1] - widths[0] / 2, pts[0][0] + widths[0] / 2, pts[0][1] + widths[0] / 2],
        fill=255,
    )
    td.ellipse(
        [pts[-1][0] - widths[-1] / 2, pts[-1][1] - widths[-1] / 2, pts[-1][0] + widths[-1] / 2, pts[-1][1] + widths[-1] / 2],
        fill=255,
    )

# hard-clip the trunk to the pot's opening: the root-flare cap at the base
# is wider than the rim band is tall, so on its own it bulges out past the
# rim onto the pot's front face. Cutting anything below rim_y1 keeps the
# trunk strictly inside the pot -- it now reads as vanishing into the soil
# right at the rim line instead of sitting on top of the pot.
below_rim = mask_from_draw(size, lambda d: d.rectangle([0, S(rim_y1), C, C], fill=255))
trunk_mask_img = ImageChops.subtract(trunk_mask_img, below_rim)

trunk_grad = linear_gradient(
    size, (cx - S(60), S(180)), (cx + S(40), S(360)), (150, 106, 70, 255), (66, 42, 28, 255)
)
trunk_layer = Image.new("RGBA", size, (0, 0, 0, 0))
trunk_layer.paste(trunk_grad, (0, 0), trunk_mask_img)
img.alpha_composite(trunk_layer)

# soften the trunk/soil junction: the trunk's rounded base cap and the
# soil ellipse are two flat-ish shapes meeting at a hard seam right where
# the trunk "enters the pot" -- a small soft dark contact-shadow blob
# right at that seam sells it as one continuous surface instead of two
# separate stickers overlapping.
junction = mask_from_draw(
    size,
    lambda d: d.ellipse(
        [cx - S(28), soil_cy - S(10), cx + S(26), soil_cy + S(16)], fill=255
    ),
)
junction = junction.filter(ImageFilter.GaussianBlur(S(7)))
junction = ImageChops.multiply(junction, rim_mask)  # never bleed past the rim band
img.paste(Image.new("RGBA", size, (18, 11, 7, 150)), (0, 0), junction)

# bark highlight: a thin lighter stripe along the sunlit side of the main trunk
bark_hl_mask = Image.new("L", size, 0)
bhd = ImageDraw.Draw(bark_hl_mask)
bhd.line([(cx + S(6), soil_cy - S(4)), (cx + S(30), S(300)), (cx + S(4), S(232))], fill=255, width=S(6), joint="curve")
bark_hl_mask = bark_hl_mask.filter(ImageFilter.GaussianBlur(S(2)))
bark_hl_mask = Image.composite(bark_hl_mask, Image.new("L", size, 0), trunk_mask_img)
img.paste(Image.new("RGBA", size, (196, 156, 112, 130)), (0, 0), bark_hl_mask)

# bark shadow: darker stripe on the shaded side
bark_sh_mask = Image.new("L", size, 0)
bsd = ImageDraw.Draw(bark_sh_mask)
bsd.line([(cx - S(14), soil_cy - S(2)), (cx + S(16), S(302)), (cx - S(12), S(226))], fill=255, width=S(7), joint="curve")
bark_sh_mask = bark_sh_mask.filter(ImageFilter.GaussianBlur(S(2)))
bark_sh_mask = Image.composite(bark_sh_mask, Image.new("L", size, 0), trunk_mask_img)
img.paste(Image.new("RGBA", size, (30, 18, 12, 110)), (0, 0), bark_sh_mask)

# ---------------------------------------------------------------- canopy
# (center, rx, ry) in 512-space, back-to-front draw order
blobs = [
    (cx - S(88), S(178), S(72), S(66)),
    (cx + S(46), S(150), S(88), S(74)),
    (cx - S(4), S(200), S(70), S(62)),
    (cx - S(70), S(190), S(58), S(52)),
    (cx + S(40), S(160), S(64), S(56)),
]

canopy_dark = (28, 64, 36, 255)
canopy_light = (168, 210, 110, 255)

for bx, by, rx, ry in blobs:
    bmask = mask_from_draw(size, lambda d, bx=bx, by=by, rx=rx, ry=ry: d.ellipse(
        [bx - rx, by - ry, bx + rx, by + ry], fill=255
    ))
    bgrad = radial_gradient(
        size, (bx - rx * 0.5, by - ry * 0.6), max(rx, ry) * 1.05, canopy_light, canopy_dark
    )
    blayer = Image.new("RGBA", size, (0, 0, 0, 0))
    blayer.paste(bgrad, (0, 0), bmask)
    img.alpha_composite(blayer)

# contact shadow where the canopy sits on/around the trunk top
canopy_contact = mask_from_draw(size, lambda d: d.ellipse(
    [cx - S(50), S(210), cx + S(50), S(250)], fill=255
))
canopy_contact = canopy_contact.filter(ImageFilter.GaussianBlur(S(10)))
img.paste(Image.new("RGBA", size, (10, 24, 14, 90)), (0, 0), canopy_contact)

# unify the whole canopy with one soft, small specular highlight (upper-left)
canopy_union = Image.new("L", size, 0)
cud = ImageDraw.Draw(canopy_union)
for bx, by, rx, ry in blobs:
    cud.ellipse([bx - rx, by - ry, bx + rx, by + ry], fill=255)
gloss = radial_gradient(
    size, (cx - S(70), S(115)), S(110), (255, 255, 230, 80), (255, 255, 230, 0)
)
gloss_layer = Image.new("RGBA", size, (0, 0, 0, 0))
gloss_layer.paste(gloss, (0, 0), canopy_union)
img.alpha_composite(gloss_layer)

# ---------------------------------------------------------------- final gloss sweep over the whole icon
sweep = radial_gradient(size, (S(120), S(90)), S(420), (255, 255, 255, 22), (255, 255, 255, 0))
sweep_layer = Image.new("RGBA", size, (0, 0, 0, 0))
sweep_layer.paste(sweep, (0, 0), bg_mask)
img.alpha_composite(sweep_layer)

# downsample while fully opaque (no transparency yet -> no Lanczos edge halo),
# then cut the rounded-rect corners as a clean alpha mask at the very end.
img_512 = img.convert("RGB").resize((SIZE, SIZE), Image.LANCZOS)
corner_mask = bg_mask.resize((SIZE, SIZE), Image.LANCZOS)
img_512 = img_512.convert("RGBA")
img_512.putalpha(corner_mask)

ASSETS = os.path.dirname(os.path.abspath(__file__))
os.makedirs(f"{ASSETS}/icons", exist_ok=True)

img_512.save(f"{ASSETS}/icon.png")
for s in (16, 32, 48, 64, 128, 256, 512):
    img_512.resize((s, s), Image.LANCZOS).save(f"{ASSETS}/icons/icon-{s}.png")
img_512.save(f"{ASSETS}/icon.ico", sizes=[(s, s) for s in (16, 32, 48, 64, 128, 256)])

print(f"wrote icon.png, icon.ico, icons/icon-*.png to {ASSETS}")
