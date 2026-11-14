"""Packs the logo PNGs that `oh-key --render-png logo <folder>` writes into
crates/oh-key-app/assets/oh-key.ico, the icon embedded in oh-key.exe.

    uv run --no-project python tools/make-icon.py <folder>
"""

import struct
import sys
from pathlib import Path

ICON_SIZES = [16, 20, 24, 32, 48, 64, 256]
OUTPUT = Path(__file__).resolve().parent.parent / "crates/oh-key-app/assets/oh-key.ico"


def main() -> None:
    folder = Path(sys.argv[1])
    images = [(folder / f"logo-{size}.png").read_bytes() for size in ICON_SIZES]
    header = struct.pack("<HHH", 0, 1, len(images))
    directory = b""
    offset = len(header) + 16 * len(images)
    for size, image in zip(ICON_SIZES, images):
        # A width or height of 256 is written as 0.
        edge = size % 256
        directory += struct.pack("<BBBBHHII", edge, edge, 0, 0, 1, 32, len(image), offset)
        offset += len(image)
    OUTPUT.write_bytes(header + directory + b"".join(images))
    print(f"{OUTPUT} ({OUTPUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
