from __future__ import annotations

import argparse
import math
from pathlib import Path

import fitz
from PIL import Image, ImageDraw


def render_pages(pdf_path: Path, out_dir: Path, dpi: int) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    doc = fitz.open(pdf_path)
    scale = dpi / 72.0
    matrix = fitz.Matrix(scale, scale)

    thumbs: list[Image.Image] = []
    for index, page in enumerate(doc):
        pix = page.get_pixmap(matrix=matrix, alpha=False)
        path = out_dir / f"page-{index + 1:02}.png"
        pix.save(path)

        image = Image.open(path).convert("RGB")
        image.thumbnail((240, 320))
        canvas = Image.new("RGB", (260, 350), "white")
        canvas.paste(image, ((260 - image.width) // 2, 20))
        draw = ImageDraw.Draw(canvas)
        draw.text((10, 5), f"page {index + 1}", fill=(0, 0, 0))
        thumbs.append(canvas)

    columns = 4
    rows = math.ceil(len(thumbs) / columns)
    sheet = Image.new("RGB", (columns * 260, rows * 350), (235, 235, 235))
    for index, thumb in enumerate(thumbs):
        x = (index % columns) * 260
        y = (index // columns) * 350
        sheet.paste(thumb, (x, y))
    sheet.save(out_dir / "contact-sheet.png")


def crop(page_path: Path, out_path: Path, box: tuple[float, float, float, float]) -> None:
    image = Image.open(page_path).convert("RGB")
    left = int(round(box[0] * image.width))
    top = int(round(box[1] * image.height))
    right = int(round(box[2] * image.width))
    bottom = int(round(box[3] * image.height))
    image.crop((left, top, right, bottom)).save(out_path)


def compare(candidate_path: Path, reference_path: Path) -> None:
    candidate = Image.open(candidate_path).convert("L")
    reference = Image.open(reference_path).convert("L")
    candidate = candidate.resize(reference.size, Image.Resampling.LANCZOS)

    c = list(candidate.getdata())
    r = list(reference.getdata())
    n = len(c)
    mse = sum((a - b) ** 2 for a, b in zip(c, r)) / n
    mae = sum(abs(a - b) for a, b in zip(c, r)) / n
    mean_c = sum(c) / n
    mean_r = sum(r) / n
    var_c = sum((a - mean_c) ** 2 for a in c) / n
    var_r = sum((a - mean_r) ** 2 for a in r) / n
    cov = sum((a - mean_c) * (b - mean_r) for a, b in zip(c, r)) / n
    c1 = (0.01 * 255) ** 2
    c2 = (0.03 * 255) ** 2
    ssim = ((2 * mean_c * mean_r + c1) * (2 * cov + c2)) / (
        (mean_c**2 + mean_r**2 + c1) * (var_c + var_r + c2)
    )

    edge_c = [value < 210 for value in c]
    edge_r = [value < 210 for value in r]
    intersection = sum(a and b for a, b in zip(edge_c, edge_r))
    union = sum(a or b for a, b in zip(edge_c, edge_r))
    edge_iou = intersection / union if union else 0.0

    print(f"candidate: {candidate_path}")
    print(f"reference: {reference_path}")
    print(f"size: {reference.width}x{reference.height}")
    print(f"mse: {mse:.3f}")
    print(f"mae: {mae:.3f}")
    print(f"ssim_global: {ssim:.4f}")
    print(f"edge_iou@210: {edge_iou:.4f}")


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)

    render = sub.add_parser("render")
    render.add_argument("--pdf", type=Path, required=True)
    render.add_argument("--out", type=Path, required=True)
    render.add_argument("--dpi", type=int, default=144)

    crop_cmd = sub.add_parser("crop")
    crop_cmd.add_argument("--page", type=Path, required=True)
    crop_cmd.add_argument("--out", type=Path, required=True)
    crop_cmd.add_argument("--box", type=float, nargs=4, required=True)

    compare_cmd = sub.add_parser("compare")
    compare_cmd.add_argument("--candidate", type=Path, required=True)
    compare_cmd.add_argument("--reference", type=Path, required=True)

    args = parser.parse_args()
    if args.cmd == "render":
        render_pages(args.pdf, args.out, args.dpi)
    elif args.cmd == "crop":
        crop(args.page, args.out, tuple(args.box))
    elif args.cmd == "compare":
        compare(args.candidate, args.reference)


if __name__ == "__main__":
    main()
