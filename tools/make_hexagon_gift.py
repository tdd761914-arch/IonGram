"""Generate IonGram's original vector-only, looping 512px TGS gift (MIT)."""
import gzip
import json
import math
from pathlib import Path


def prop(value):
    return {"a": 0, "k": value}


def animated(values):
    frames = []
    for i, (time, value) in enumerate(values):
        frame = {"t": time, "s": value}
        if i + 1 < len(values):
            frame.update(e=values[i + 1][1], i={"x": [0.6], "y": [1]}, o={"x": [0.4], "y": [0]})
        frames.append(frame)
    return {"a": 1, "k": frames}


def polygon(radius):
    vertices = [[radius * math.cos(math.radians(60 * n - 90)), radius * math.sin(math.radians(60 * n - 90))] for n in range(6)]
    return {"ty": "sh", "ks": prop({"v": vertices, "i": [[0, 0]] * 6, "o": [[0, 0]] * 6, "c": True})}


def fill(color):
    return {"ty": "fl", "c": prop(color), "o": prop(100), "r": 1}


def stroke(color, width):
    return {"ty": "st", "c": prop(color), "o": prop(100), "w": prop(width), "lc": 2, "lj": 2}


def layer(index, name, shapes, rotation=0, position=None, scale=None):
    return {"ddd": 0, "ind": index, "ty": 4, "nm": name, "sr": 1,
            "ks": {"o": prop(100), "r": rotation if isinstance(rotation, dict) else prop(rotation), "p": position or prop([256, 256, 0]), "a": prop([0, 0, 0]), "s": scale or prop([100, 100, 100])},
            "ao": 0, "shapes": shapes, "ip": 0, "op": 120, "st": 0, "bm": 0}


rotation = animated([(0, [0]), (60, [12]), (120, [0])])
bob = animated([(0, [256, 265, 0]), (60, [256, 245, 0]), (120, [256, 265, 0])])
layers = [
    layer(1, "Gold inner hexagon", [polygon(87), fill([1, .77, .25, 1]), stroke([1, .94, .65, 1], 6)], rotation, bob),
    layer(2, "Turquoise bevel", [polygon(140), fill([.08, .48, .61, 1]), stroke([.44, .98, 1, 1], 13)], rotation, bob),
    layer(3, "Deep blue outer rim", [polygon(163), fill([.025, .14, .29, 1]), stroke([.12, .72, .87, 1], 5)], rotation, bob),
    layer(4, "Orbit", [polygon(201), stroke([.13, .52, .62, 1], 3)], animated([(0, [0]), (120, [60])])),
]
for i, (x, y) in enumerate([(85, 105), (418, 134), (422, 382), (88, 378)]):
    star = {"ty": "sr", "sy": 1, "d": 1, "pt": prop(4), "p": prop([0, 0]), "r": prop(0), "ir": prop(4), "or": prop(15), "is": prop(0), "os": prop(0)}
    layers.insert(0, layer(5 + i, "Sparkle", [star, fill([1, .88, .4, 1])], position=prop([x, y, 0]), scale=animated([(0, [70, 70, 100]), (60, [120, 120, 100]), (120, [70, 70, 100])])))
animation = {"tgs": 1, "v": "5.7.4", "fr": 60, "ip": 0, "op": 120, "w": 512, "h": 512, "nm": "IonGram Hexagon", "ddd": 0, "assets": [], "layers": layers}
root = Path(__file__).resolve().parent.parent
(root / "assets/gifts/hexagon.tgs").write_bytes(gzip.compress(json.dumps(animation, separators=(",", ":")).encode(), mtime=0))
print(root / "assets/gifts/hexagon.tgs")
