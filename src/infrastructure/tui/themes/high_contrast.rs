use super::Scheme;

pub(super) const SCHEME: Scheme = Scheme {
    text: 0xffffff,
    soft: 0xe0e0e0,
    muted: 0xa8a8a8,
    accent: 0x00ffff,
    ok: 0x00ff00,
    warn: 0xffff00,
    err: 0xff4040,
    info: 0x4da6ff,
    special: 0xff66ff,
    glow: 0xffd700,
    base: 0x000000,
    select: 0x0037da,
    header: 0x1a1a1a,
    void: 0x000000,
    heat: [0x1a1a1a, 0x005f00, 0x00a000, 0x00d700, 0x00ff00],
};
