"""Generate the original monochrome hexagon pattern and three RGB backdrops."""
import gzip
import json
import math
from pathlib import Path

root = Path(__file__).resolve().parent.parent
vertices = [[150 * math.cos(math.radians(60 * n - 90)), 150 * math.sin(math.radians(60 * n - 90))] for n in range(6)]
shape = {"ty": "sh", "ks": {"a": 0, "k": {"v": vertices, "i": [[0, 0]] * 6, "o": [[0, 0]] * 6, "c": True}}}
layer = {"ddd": 0, "ind": 1, "ty": 4, "nm": "Hexagon pattern symbol", "sr": 1, "ks": {"o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0}, "p": {"a": 0, "k": [256, 256, 0]}, "a": {"a": 0, "k": [0, 0, 0]}, "s": {"a": 0, "k": [100, 100, 100]}}, "shapes": [shape, {"ty": "st", "c": {"a": 0, "k": [1, 1, 1, 1]}, "o": {"a": 0, "k": 100}, "w": {"a": 0, "k": 25}, "lc": 2, "lj": 2}], "ip": 0, "op": 60, "st": 0, "bm": 0}
anim = {"tgs": 1, "v": "5.7.4", "fr": 60, "ip": 0, "op": 60, "w": 512, "h": 512, "nm": "Hexagon", "ddd": 0, "assets": [], "layers": [layer]}
(root / "assets/gifts/hexagon-pattern.tgs").write_bytes(gzip.compress(json.dumps(anim, separators=(",", ":")).encode(), mtime=0))
backgrounds = []
for id_, name, center, edge, pattern, text, rarity in [
    (12701, "Чёрный", 0x282B30, 0x08090B, 0x50545C, 0xFFFFFF, 334),
    (12702, "Красный", 0xEF5350, 0x8E1527, 0xFF8C88, 0xFFFFFF, 333),
    (12703, "Зелёный", 0x46B878, 0x145E3D, 0x8BE1AB, 0xFFFFFF, 333),
]:
    backgrounds.append({"_": "starGiftAttributeBackdrop", "name": name, "backdrop_id": id_, "center_color": center, "edge_color": edge, "pattern_color": pattern, "text_color": text, "rarity": {"_": "starGiftAttributeRarity", "permille": rarity}})
(root / "assets/gifts/hexagon-backgrounds.json").write_text(json.dumps(backgrounds, ensure_ascii=False, indent=2) + "\n")
