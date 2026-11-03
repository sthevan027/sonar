"""Gera o ícone do Sonar (PNGs, .ico e versão da bandeja) com Pillow.

Uso:  python tools/make_icons.py

As medidas seguem o SVG em assets/icon/sonar.svg (viewBox 512). Desenha em 4x e reduz,
para ficar sem serrilhado. Saída em assets/icon/.
"""
from pathlib import Path
from PIL import Image, ImageChops, ImageDraw, ImageOps

OUT = Path(__file__).resolve().parent.parent / "assets" / "icon"
SS = 4                      # supersampling
U = 512 * SS                # tela de trabalho
BRASS_TOP, BRASS_BOT = "#F3C86B", "#C98F2E"


def s(v):
    return v * SS


def vgrad(box_y0, box_y1, c0, c1):
    """Gradiente vertical do tamanho da tela, entre y0 e y1 (em unidades 512)."""
    g = Image.linear_gradient("L").resize((U, U))          # preto em cima, branco embaixo (0..U)
    # remapeia para que 0 fique em y0 e 255 em y1
    lut = []
    for i in range(256):
        y = i / 255 * 512
        t = min(max((y - box_y0) / (box_y1 - box_y0), 0), 1)
        lut.append(int(t * 255))
    g = g.point(lut)
    return ImageOps.colorize(g, c0, c1).convert("RGBA")


def hgrad(x0, x1, c0, c1):
    g = Image.linear_gradient("L").rotate(90, expand=True).transpose(Image.FLIP_LEFT_RIGHT).resize((U, U))
    lut = []
    for i in range(256):
        x = i / 255 * 512
        t = min(max((x - x0) / (x1 - x0), 0), 1)
        lut.append(int(t * 255))
    g = g.point(lut)
    return ImageOps.colorize(g, c0, c1).convert("RGBA")


def arc_mask(cx, cy, radii, stroke, half_gap_deg=40, dot_r=None):
    """Máscara dos arcos abertos embaixo (extremidades arredondadas) + ponto central."""
    m = Image.new("L", (U, U), 0)
    d = ImageDraw.Draw(m)
    import math
    for r in radii:
        ro = r + stroke / 2     # o Pillow desenha a espessura para dentro do raio: usa o raio externo
        box = [s(cx - ro), s(cy - ro), s(cx + ro), s(cy + ro)]
        d.arc(box, start=90 + half_gap_deg, end=90 - half_gap_deg, fill=255, width=int(s(stroke)))
        for sign in (-1, 1):
            ex = cx + sign * r * math.sin(math.radians(half_gap_deg))
            ey = cy + r * math.cos(math.radians(half_gap_deg))
            d.ellipse([s(ex - stroke / 2), s(ey - stroke / 2), s(ex + stroke / 2), s(ey + stroke / 2)], fill=255)
    if dot_r:
        d.ellipse([s(cx - dot_r), s(cy - dot_r), s(cx + dot_r), s(cy + dot_r)], fill=255)
    return m


def squircle_bg():
    # fundo grafite com brilho radial suave e fio de borda
    base = Image.new("RGBA", (U, U), (0, 0, 0, 0))
    # raio do gradiente = 75% de 480 = 360 unidades; centro em (50%, 42%) do quadrado
    side = int(s(720))
    rg = Image.radial_gradient("L").resize((side, side))      # 0 no centro, 255 na borda
    col = ImageOps.colorize(rg, "#2B2823", "#1B1A18").convert("RGBA")
    layer = Image.new("RGBA", (U, U), (0, 0, 0, 0))
    layer.paste(col, (int(s(256)) - side // 2, int(s(217.6)) - side // 2))
    mask = Image.new("L", (U, U), 0)
    ImageDraw.Draw(mask).rounded_rectangle([s(16), s(16), s(496), s(496)], radius=s(116), fill=255)
    base.paste(layer, (0, 0), mask)
    ring = Image.new("RGBA", (U, U), (0, 0, 0, 0))
    ImageDraw.Draw(ring).rounded_rectangle([s(17.5), s(17.5), s(494.5), s(494.5)], radius=s(114.5),
                                           outline="#37332B", width=int(s(3)))
    base.alpha_composite(ring)
    return base


def build_app_icon():
    img = squircle_bg()
    mask = arc_mask(256, 248, (54, 104, 154), 21, dot_r=20)
    brass = vgrad(80, 380, BRASS_TOP, BRASS_BOT)
    img.paste(brass, (0, 0), mask)
    # barra de volume: trilho em latão + botão branco à esquerda
    track = Image.new("L", (U, U), 0)
    ImageDraw.Draw(track).rounded_rectangle([s(168), s(392), s(344), s(416)], radius=s(12), fill=255)
    img.paste(hgrad(168, 344, "#E5B454", "#D9A441"), (0, 0), track)
    knob = Image.new("L", (U, U), 0)
    ImageDraw.Draw(knob).rounded_rectangle([s(168), s(392), s(268), s(416)], radius=s(12), fill=255)
    img.paste(Image.new("RGBA", (U, U), "#F7F4EC"), (0, 0), knob)
    return img


def build_tray_icon(color="#E3B04F"):
    """Versão da bandeja: sem fundo, arcos mais grossos, sem barra de volume."""
    img = Image.new("RGBA", (U, U), (0, 0, 0, 0))
    mask = arc_mask(256, 277, (66, 128, 190), 38, dot_r=30)
    img.paste(Image.new("RGBA", (U, U), color), (0, 0), mask)
    return img


def save_sizes(img, folder, prefix, sizes):
    folder.mkdir(parents=True, exist_ok=True)
    for n in sizes:
        img.resize((n, n), Image.LANCZOS).save(folder / f"{prefix}{n}.png")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    app = build_app_icon()
    save_sizes(app, OUT / "png", "sonar-", (1024, 512, 256, 128, 64, 48, 32, 24, 16))
    # nomes que o Tauri espera
    app.resize((1024, 1024), Image.LANCZOS).save(OUT / "icon.png")
    app.resize((32, 32), Image.LANCZOS).save(OUT / "32x32.png")
    app.resize((128, 128), Image.LANCZOS).save(OUT / "128x128.png")
    app.resize((256, 256), Image.LANCZOS).save(OUT / "128x128@2x.png")
    big = app.resize((256, 256), Image.LANCZOS)
    big.save(OUT / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

    tray = build_tray_icon()
    save_sizes(tray, OUT / "tray", "sonar-tray-", (64, 48, 32, 24, 16))
    tray.resize((32, 32), Image.LANCZOS).save(OUT / "tray.png")
    # variante escura para a barra de tarefas do tema claro
    tray_light = build_tray_icon("#1B1A18")
    save_sizes(tray_light, OUT / "tray-light", "sonar-tray-light-", (64, 48, 32, 24, 16))
    tray_light.resize((32, 32), Image.LANCZOS).save(OUT / "tray-light.png")
    print("ok ->", OUT)


if __name__ == "__main__":
    main()
