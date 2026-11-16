"""Upscale lachy's strips x2 for make-pack --upscale: Real-ESRGAN's anime video model (x4) per cell, then
Lanczos down to x2. CPU only, fixed thread count, deterministic algorithms: the same torch on the same CPU
gives the same pixels, so make-pack can pin them (tools/lachy-x2.sha256).

The network shape (SRVGGNetCompact) follows Real-ESRGAN, https://github.com/xinntao/Real-ESRGAN
(BSD-3-Clause, Copyright (c) 2021 Xintao Wang); the weights are its v0.2.5.0 release asset, sha256-pinned.
Needs torch, numpy, scipy and Pillow (only for --upscale)."""
import hashlib, io, urllib.request
from pathlib import Path

MODEL_URL = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-animevideov3.pth"
MODEL_SHA = "b8a8376811077954d82ca3fcf476f1ac3da3e8a68a4f4d71363008000a18b75d"
FACTOR = 2
THREADS = 4
PAD = 8


def pixel_sha(im):
    """sha256 of the decoded pixels (size + RGBA bytes): independent of the PNG encoder."""
    im = im.convert("RGBA")
    return hashlib.sha256(f"{im.width}x{im.height}\n".encode() + im.tobytes()).hexdigest()


def model_bytes(cache):
    p = Path(cache) / "realesr-animevideov3.pth"
    if p.is_file() and hashlib.sha256(p.read_bytes()).hexdigest() == MODEL_SHA:
        return p.read_bytes()
    with urllib.request.urlopen(MODEL_URL, timeout=60) as f:
        b = f.read()
    if hashlib.sha256(b).hexdigest() != MODEL_SHA:
        raise SystemExit(f"make-pack: {MODEL_URL} does not match its pinned sha256")
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(b)
    return b


def load_net(cache):
    import torch, torch.nn as nn, torch.nn.functional as F

    class Compact(nn.Module):
        def __init__(s, nf=64, nc=16, up=4):
            super().__init__()
            s.up = up
            b = [nn.Conv2d(3, nf, 3, 1, 1), nn.PReLU(nf)]
            for _ in range(nc):
                b += [nn.Conv2d(nf, nf, 3, 1, 1), nn.PReLU(nf)]
            b.append(nn.Conv2d(nf, 3 * up * up, 3, 1, 1))
            s.body = nn.ModuleList(b)
            s.ps = nn.PixelShuffle(up)

        def forward(s, x):
            o = x
            for m in s.body:
                o = m(o)
            return s.ps(o) + F.interpolate(x, scale_factor=s.up, mode="nearest")

    torch.set_num_threads(THREADS)
    torch.use_deterministic_algorithms(True)
    sd = torch.load(io.BytesIO(model_bytes(cache)), map_location="cpu", weights_only=True)
    net = Compact()
    net.load_state_dict(sd.get("params", sd), strict=True)
    return net.eval()


def upscale_cell(net, cell):
    """RGBA PIL cell -> RGBA PIL cell at FACTOR x. RGB is edge-bled under transparent px (no dark fringe);
    alpha goes through the net as grey."""
    import numpy as np, torch, torch.nn.functional as F
    from scipy import ndimage
    from PIL import Image

    a = np.asarray(cell.convert("RGBA"))
    solid = a[..., 3] > 0
    rgb = a[..., :3]
    if solid.any() and not solid.all():
        idx = ndimage.distance_transform_edt(~solid, return_distances=False, return_indices=True)
        rgb = rgb[idx[0], idx[1]]
    outs = []
    with torch.no_grad():
        for arr in (rgb.astype(np.float32) / 255, np.repeat(a[..., 3:4], 3, 2).astype(np.float32) / 255):
            t = F.pad(torch.from_numpy(np.ascontiguousarray(arr)).permute(2, 0, 1)[None], (PAD,) * 4, mode="replicate")
            o = net(t).clamp(0, 1)[0, :, 4 * PAD:-4 * PAD, 4 * PAD:-4 * PAD]
            outs.append(o.permute(1, 2, 0).numpy())
    x4 = np.concatenate([outs[0], outs[1].mean(2, keepdims=True)], 2)
    big = Image.fromarray((x4 * 255 + 0.5).astype(np.uint8), "RGBA")
    return big.resize((cell.width * FACTOR, cell.height * FACTOR), Image.LANCZOS)


def upscale_strip(net, path, cell_w, frames):
    from PIL import Image

    im = Image.open(path).convert("RGBA")
    out = Image.new("RGBA", (im.width * FACTOR, im.height * FACTOR))
    for i in range(frames):
        c = im.crop((i * cell_w, 0, (i + 1) * cell_w, im.height))
        out.paste(upscale_cell(net, c), (i * cell_w * FACTOR, 0))
    return out
