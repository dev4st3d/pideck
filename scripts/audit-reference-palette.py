#!/usr/bin/env python3
"""Sample the supplied reference PNGs against source palette values, not native output."""
from __future__ import annotations
import hashlib
import json
import re
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / 'docs' / 'reference' / 'images'
SAMPLES = {
    'canvas': (1300, 600),
    'floor': (280, 700),
    'rail': (20, 700),
    'panel': (1250, 850),
    'user_message': (460, 210),
    'selection': (95, 428),
    'signal': (95, 225),
    'edge': (600, 39),
    'search_surface': (95, 255),
}
THEMES = ['ORIGINAL', 'ORIGINAL', 'ORIGINAL', 'LINEN', 'GRAPHITE', 'MIDNIGHT']


def main() -> int:
    source = (ROOT / 'src/theme.rs').read_text(encoding='utf8')
    references = sorted(REFERENCE.glob('*.png'))
    if len(references) != 6:
        raise ValueError('All six original reference PNGs are required')
    report = {'scope': 'Flat reference pixels versus source constants. NOT a native screenshot comparison.', 'screens': []}
    for path, theme in zip(references, THEMES):
        match = re.search(r'const ' + theme + r': Palette = Palette \{([\s\S]*?)\n\};', source)
        if not match:
            raise ValueError(f'Missing palette {theme}')
        palette = dict(re.findall(r'(\w+):\s*0x([a-f0-9]{8})', match[1]))
        with Image.open(path) as image:
            image = image.convert('RGB')
            if image.size != (1440, 960):
                raise ValueError(f'Wrong reference size: {path.name}')
            results = []
            for role, point in SAMPLES.items():
                sampled = '%02x%02x%02x' % image.getpixel(point)
                expected = palette[role][:6]
                results.append({'role': role, 'point': list(point), 'reference_rgb': sampled,
                                'source_rgb': expected, 'equal': sampled == expected})
        report['screens'].append({'name': path.name, 'theme': theme,
                                  'reference_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'samples': results})
    report['all_flat_samples_equal'] = all(s['equal'] for screen in report['screens'] for s in screen['samples'])
    print(json.dumps(report, indent=2))
    return 0 if report['all_flat_samples_equal'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
