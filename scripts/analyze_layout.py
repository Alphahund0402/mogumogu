#!/usr/bin/env python3
"""Inspect native Slint layout_audit exports; standard library only.

Produces lossless PNGs, all component bounds/insets, sibling gaps and review
candidates. Text boxes are Slint's shaped layout metrics, not pixel ink masks.
Clipped scrolling content and hidden elements are excluded from collisions.
Candidates require visual review; layered graphics can overlap intentionally.
"""

import argparse
import csv
import json
from pathlib import Path
import struct
import zlib


def png_from_ppm(source):
    with source.open('rb') as stream:
        assert stream.readline() == b'P6\n'
        width, height = map(int, stream.readline().split())
        assert stream.readline() == b'255\n'
        pixels = stream.read()
    assert len(pixels) == width * height * 3

    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))

    stride = width * 3
    scanlines = b''.join(b'\0' + pixels[y * stride:(y + 1) * stride] for y in range(height))
    source.with_suffix('.png').write_bytes(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 2, 0, 0, 0))
        + chunk(b'IDAT', zlib.compress(scanlines))
        + chunk(b'IEND', b''))


def rectangle(node):
    return (node['x'], node['y'], node['x'] + node['width'], node['y'] + node['height'])


def intersection(first, second):
    result = (max(first[0], second[0]), max(first[1], second[1]),
              min(first[2], second[2]), min(first[3], second[3]))
    return result if result[2] > result[0] and result[3] > result[1] else None


def label(node):
    return ' / '.join(f"{element['type']}::{element['id']}" for element in node['elements'])


def write_csv(path, rows, fields):
    with path.open('w', encoding='utf-8', newline='') as stream:
        writer = csv.DictWriter(stream, fields, extrasaction='ignore')
        writer.writeheader()
        writer.writerows(rows)


def analyze(record):
    bounds, gaps, texts, candidates = [], [], [], []
    viewport = (0, 0, record['physical_width'] / record['scale'], record['physical_height'] / record['scale'])
    if abs(record['tree']['width'] - viewport[2]) > .75 or abs(record['tree']['height'] - viewport[3]) > .75 or abs(record['logical_width'] - viewport[2]) > .75:
        candidates.append(dict(kind='unexpected-window-size', state=record['name'],
                               expected=viewport[2:], actual=(record['tree']['width'], record['tree']['height']),
                               stored_width=record['logical_width']))

    def visit(node, path, parent, clip, layers, scrolling=False, hidden=False):
        if any(node[key] is None for key in ('x', 'y', 'width', 'height')):
            # Slint's hidden zero-range standard scrollbar thumb has NaN x/y.
            # Preserve it in raw JSON; a visible non-finite rectangle is a bug.
            if node['visible']:
                candidates.append(dict(kind='non-finite-geometry', state=record['name'], path=path, component=label(node)))
            return
        rect = rectangle(node)
        name = label(node)
        if any(kind in name for kind in ('DialogLayer::', 'GuideOverlay::', 'SearchPopup::')):
            layers += (path,)
        hidden = hidden or (node.get('visibility_clip', False) and node['clips_children'])
        # Offscreen content inside a ScrollView/ListView is expected. Controls
        # outside the actual window without such an ancestor are not reachable.
        if not hidden and not scrolling and ('text' in node or 'input' in node or any(kind in name for kind in ('UiButton::', 'PillField::', 'FilterChip::', 'NavItem::'))):
            if rect[0] < -.75 or rect[1] < -.75 or rect[2] > viewport[2]+.75 or rect[3] > viewport[3]+.75:
                candidates.append(dict(kind='outside-window', state=record['name'], path=path,
                                       component=name, rect=rect, text=node.get('text', {}).get('value', '')))
            elif not clip or rect[0] < clip[0]-.75 or rect[1] < clip[1]-.75 or rect[2] > clip[2]+.75 or rect[3] > clip[3]+.75:
                candidates.append(dict(kind='static-parent-clipping', state=record['name'], path=path,
                                       component=name, rect=rect, clip=clip, text=node.get('text', {}).get('value', '')))
        scrolling = scrolling or node.get('scroll_container', False)
        if node['clips_children']:
            clip = intersection(clip, rect) if clip else None
        visible = bool(not hidden and node['visible'] and clip and intersection(clip, rect))
        if not hidden and parent and node['width'] > 0 and node['height'] > 0 and ('text' in node or 'input' in node or any(kind in name for kind in ('StatusChip::', 'UiButton::', 'PillField::', 'FilterChip::', 'NavItem::'))):
            if rect[0] < parent[0]-.75 or rect[1] < parent[1]-.75 or rect[2] > parent[2]+.75 or rect[3] > parent[3]+.75:
                candidates.append(dict(kind='component-outside-parent', state=record['name'], path=path,
                                       component=name, rect=rect, parent=parent, text=node.get('text', {}).get('value', '')))
        row = dict(state=record['name'], path=path, component=name, visible=visible,
                   x=node['x'], y=node['y'], width=node['width'], height=node['height'])
        if parent:
            row.update(inset_left=rect[0]-parent[0], inset_top=rect[1]-parent[1],
                       inset_right=parent[2]-rect[2], inset_bottom=parent[3]-rect[3])
        bounds.append(row)
        if visible and 'input' in node and node['input']['preferred_height'] > node['height'] + .75:
            candidates.append(dict(kind='input-height', state=record['name'], path=path, component=name,
                                   actual=node['height'], required=node['input']['preferred_height']))
        if visible and 'text' in node and node['text']['value']:
            text = node['text']
            text_rect = rectangle(text)
            clipped = intersection(text_rect, clip)
            if clipped:
                entry = dict(row, text=text['value'], rect=clipped, layer=layers,
                             preferred_width=text['preferred_width'], preferred_height=text['preferred_height'],
                             overflow=text['overflow'], wrap=text['wrap'])
                texts.append(entry)
                # A partially visible list row is expected to be clipped by its viewport.
                fully_in_clip = rect[0] >= clip[0]-.6 and rect[1] >= clip[1]-.6 and rect[2] <= clip[2]+.6 and rect[3] <= clip[3]+.6
                if fully_in_clip and text['preferred_height'] > node['height'] + .75:
                    candidates.append(dict(kind='text-height', state=record['name'], path=path, component=name,
                                           text=text['value'], actual=node['height'], required=text['preferred_height']))
                if fully_in_clip and text['overflow'] != 'Elide' and max(text['min_width'], text['preferred_width'] if text['wrap'] == 'NoWrap' else 0) > node['width'] + .75:
                    candidates.append(dict(kind='text-width', state=record['name'], path=path, component=name,
                                           text=text['value'], actual=node['width'], required=max(text['min_width'], text['preferred_width'] if text['wrap'] == 'NoWrap' else 0)))
        children = node['children']
        # Slint's visible conditional wrapper has zero geometry but does not
        # clip or lay out its children. Keep the real parent's bounds instead
        # of treating this transparent implementation node as a 0x0 container.
        child_parent = parent if node.get('visibility_clip', False) and not node['clips_children'] else rect
        for index, child in enumerate(children):
            visit(child, f'{path}.{index}', child_parent, clip, layers, scrolling, hidden)
        for index, first in enumerate(children):
            if not first['visible'] or any(first[key] is None for key in ('x', 'y', 'width', 'height')):
                continue
            a = rectangle(first)
            for second_index, second in enumerate(children[index + 1:], index + 1):
                if not second['visible'] or any(second[key] is None for key in ('x', 'y', 'width', 'height')):
                    continue
                b = rectangle(second)
                if min(a[3], b[3]) > max(a[1], b[1]) and (a[2] <= b[0] or b[2] <= a[0]):
                    gaps.append(dict(state=record['name'], parent=path, first=index, second=second_index,
                                     axis='horizontal', gap=max(b[0]-a[2], a[0]-b[2])))
                if min(a[2], b[2]) > max(a[0], b[0]) and (a[3] <= b[1] or b[3] <= a[1]):
                    gaps.append(dict(state=record['name'], parent=path, first=index, second=second_index,
                                     axis='vertical', gap=max(b[1]-a[3], a[1]-b[3])))

    visit(record['tree'], '0', None, viewport, ())
    collision_texts = texts
    for index, first in enumerate(collision_texts):
        for second in collision_texts[index + 1:]:
            if first['layer'] != second['layer']:
                continue
            overlap = intersection(first['rect'], second['rect'])
            if overlap and overlap[2]-overlap[0] > 1 and overlap[3]-overlap[1] > 1:
                candidates.append(dict(kind='text-intersection', state=record['name'], path=first['path'],
                                       other_path=second['path'], component=first['component'], text=first['text'],
                                       other_text=second['text'], overlap_width=overlap[2]-overlap[0], overlap_height=overlap[3]-overlap[1]))
    return bounds, gaps, texts, candidates


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', nargs='?', default='target/layout-audit')
    parser.add_argument('--png', action='store_true', help='Convert native PPM renders losslessly to PNG')
    parser.add_argument('--strict', action='store_true', help='Exit with status 1 if any review candidate remains')
    args = parser.parse_args()
    directory = Path(args.directory)
    manifest = json.loads((directory / 'manifest.json').read_text(encoding='utf-8'))
    all_bounds, all_gaps, all_texts, all_candidates = [], [], [], []
    for state in manifest:
        record = json.loads((directory / f"{state['name']}.json").read_text(encoding='utf-8'))
        bounds, gaps, texts, candidates = analyze(record)
        all_bounds.extend(bounds); all_gaps.extend(gaps); all_texts.extend(texts); all_candidates.extend(candidates)
        if args.png:
            png_from_ppm(directory / f"{state['name']}.ppm")
    write_csv(directory / 'component-bounds.csv', all_bounds,
              ['state', 'path', 'component', 'visible', 'x', 'y', 'width', 'height', 'inset_left', 'inset_top', 'inset_right', 'inset_bottom'])
    write_csv(directory / 'sibling-gaps.csv', all_gaps, ['state', 'parent', 'first', 'second', 'axis', 'gap'])
    write_csv(directory / 'text-metrics.csv', all_texts,
              ['state', 'path', 'component', 'text', 'x', 'y', 'width', 'height', 'preferred_width', 'preferred_height', 'overflow', 'wrap'])
    (directory / 'review-candidates.json').write_text(json.dumps(all_candidates, ensure_ascii=False, indent=2), encoding='utf-8')
    summary = dict(states=len(manifest), components=len(all_bounds), visible_texts=len(all_texts),
                   sibling_gaps=len(all_gaps), candidates=len(all_candidates),
                   candidate_kinds={kind: sum(row['kind'] == kind for row in all_candidates) for kind in sorted({row['kind'] for row in all_candidates})})
    (directory / 'summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    print(json.dumps(summary, indent=2))
    if args.strict and all_candidates:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
