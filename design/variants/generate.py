r"""Three read-only Windows UI concepts and vector cat marks. No application I/O.

Run: python design/variants/generate.py, then .\design\variants\render.ps1.
The gallery itself opens directly from index.html; generated previews are checked in.
"""
from pathlib import Path
from html import escape
import json
import re

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]

PALETTES = {
    'a': dict(bg='#f3f6fb', surface='#ffffff', nav='#ebf0f8', ink='#223650', muted='#63748b',
              line='#dce4ef', accent='#376cce', soft='#e5edfc', good='#237f69', goodbg='#e6f4ed',
              warn='#986824', warnbg='#fff1d8', caption='#f9fbfe'),
    'b': dict(bg='#f4f5ef', surface='#fffefa', nav='#edf1e7', ink='#293e37', muted='#66756a',
              line='#dce3d8', accent='#467760', soft='#e5eddf', good='#3f735a', goodbg='#e6eee1',
              warn='#936232', warnbg='#f7ead7', caption='#fafbf6'),
    'c': dict(bg='#101a28', surface='#172436', nav='#131f30', ink='#e5edf8', muted='#a1b1c8',
              line='#2a3a51', accent='#aac3ff', soft='#283c5e', good='#8cdbbf', goodbg='#203f3b',
              warn='#edc58d', warnbg='#3d352b', caption='#0d1623'),
}

PROJECTS = [
    ('my-app', r'C:\dev\my-app', '12,4 GB', 'Belegt', 'Node.js'),
    ('website', r'D:\code\website', '856 MB', 'Belegt', 'Node.js'),
    ('ml-experiments', r'C:\dev\ml', '25,4 GB', 'Prüfen', 'Python · uv'),
    ('tools', r'C:\tools\dev', '342 MB', 'Belegt', 'Rust'),
    ('game-prototype', r'D:\projects\game', '7,6 GB', 'Unbekannt', 'C++'),
    ('old-stuff', r'D:\archive\experiments', '5,4 GB', 'Unbekannt', 'Python'),
]
NAV = [('Übersicht', 'home'), ('Projekte', 'folder'), ('Pakete', 'package'),
       ('KI-Konfiguration', 'spark'), ('Ressourcen', 'database'),
       ('Prüfliste', 'leaf'), ('Aktivität', 'activity'), ('Einstellungen', 'settings')]


def logo(kind):
    ink = '#233a59'
    opening = f'<g stroke="{ink}" stroke-linecap="round" stroke-linejoin="round">'
    if kind == 'a':
        return opening + '''
<path d="M18 39 18 20Q18 15 23 18L35 28Q48 24 61 28L73 18Q78 15 78 20L78 39Q89 49 84 64Q79 81 48 82Q17 81 12 64Q7 49 18 39Z" fill="#f8fbff" stroke-width="4.2"/>
<path d="m24 25 7 6-8 3m49-9-7 6 8 3" stroke="#c7d7f4" stroke-width="3.5" fill="none"/>
<g stroke="none"><ellipse cx="26" cy="60" rx="6" ry="3.7" fill="#efb9c6"/><ellipse cx="70" cy="60" rx="6" ry="3.7" fill="#efb9c6"/>
<ellipse cx="33" cy="50" rx="4.3" ry="5.2" fill="#233a59"/><ellipse cx="63" cy="50" rx="4.3" ry="5.2" fill="#233a59"/>
<circle cx="31.6" cy="48.4" r="1.3" fill="white"/><circle cx="61.6" cy="48.4" r="1.3" fill="white"/>
<path d="M44 59Q48 56 52 59L48 63Z" fill="#bd7c8d"/></g>
<path d="M48 63v2m-8 0q4 6 8 0 4 6 8 0" stroke-width="2.8" fill="none"/>
<path d="m19 55-11-3m10 11H6m71-8 11-3m-10 11h12" stroke-width="2.8"/>
</g>'''
    if kind == 'b':
        return opening + '''
<path d="M18 40 17 23Q17 18 22 20L34 29Q48 25 62 29L74 20Q79 18 79 23L78 40Q88 50 83 66Q77 82 48 83Q19 82 13 66Q8 50 18 40Z" fill="#fff8e9" stroke-width="4.2"/>
<path d="m23 27 6 6m44-6-6 6" stroke="#e4c7a4" stroke-width="4" fill="none"/>
<path d="M27 49q6-7 12 0m18 0q6-7 12 0" stroke-width="3.5" fill="none"/>
<g stroke="none"><ellipse cx="25" cy="60" rx="6.5" ry="4" fill="#e6b6a1"/><ellipse cx="71" cy="60" rx="6.5" ry="4" fill="#e6b6a1"/>
<path d="M44 57Q48 55 52 57L48 61Z" fill="#9c7468"/></g>
<path d="M48 61v3m-7-1q2 9 7 5 5 4 7-5" stroke-width="2.8" fill="none"/>
<path d="m17 55-9-3m9 10H6m73-7 9-3m-9 10h11" stroke-width="2.8"/>
<path d="M18 70q-6-12 3-14 9-1 10 12l1 5" fill="#fff8e9" stroke-width="3.6"/>
<path d="m22 61 2 4" fill="none" stroke-width="2"/>
</g>'''
    return opening + '''
<path d="M19 39 21 19Q22 14 26 19L37 29Q49 26 61 29L72 19Q76 15 78 20L78 40Q86 49 83 63Q79 77 48 79Q17 77 13 63Q10 49 19 39Z" fill="#f3f2ff" stroke-width="4.2"/>
<path d="m27 25 6 7m39-7-6 7" stroke="#c6c9ef" stroke-width="3.5" fill="none"/>
<g stroke="none"><ellipse cx="32" cy="49" rx="4" ry="5" fill="#233a59"/><ellipse cx="64" cy="49" rx="4" ry="5" fill="#233a59"/>
<circle cx="30.7" cy="47.5" r="1.2" fill="white"/><circle cx="62.7" cy="47.5" r="1.2" fill="white"/>
<ellipse cx="24" cy="59" rx="5.3" ry="3" fill="#c4c0e7"/><ellipse cx="72" cy="59" rx="5.3" ry="3" fill="#c4c0e7"/>
<path d="M45 57q3-2 6 0l-3 4Z" fill="#9591c5"/></g>
<path d="M48 61v2m-7 0q3 5 7 0 4 5 7 0" stroke-width="2.7" fill="none"/>
<path d="m19 55-11-3m10 10H6m71-7 11-3m-10 10h12" stroke-width="2.6"/>
<path d="M23 75q0-10 9-10t9 10v4q0 8-9 8t-9-8Zm32 0q0-10 9-10t9 10v4q0 8-9 8t-9-8Z" fill="#f3f2ff" stroke-width="3.4"/>
<path d="M29 75v4m6-4v4m26-4v4m6-4v4" stroke-width="1.9"/>
</g>'''


class Svg:
    def __init__(self, width=1080, height=680):
        self.width, self.height = width, height
        self.parts = []

    def rect(self, x, y, width, height, fill, radius=0, stroke=None, sw=1):
        border = f' stroke="{stroke}" stroke-width="{sw}"' if stroke else ''
        self.parts.append(f'<rect x="{x}" y="{y}" width="{width}" height="{height}" rx="{radius}" fill="{fill}"{border}/>')

    def text(self, x, y, value, size=12, color='#223650', weight=400, anchor=None):
        alignment = f' text-anchor="{anchor}"' if anchor else ''
        self.parts.append(f'<text x="{x}" y="{y}" font-family="Segoe UI" font-size="{size}" font-weight="{weight}" fill="{color}"{alignment}>{escape(str(value))}</text>')

    def line(self, x1, y1, x2, y2, color, sw=1):
        self.parts.append(f'<path d="M{x1} {y1}L{x2} {y2}" stroke="{color}" stroke-width="{sw}" fill="none" stroke-linecap="round"/>')

    def circle(self, x, y, radius, color):
        self.parts.append(f'<circle cx="{x}" cy="{y}" r="{radius}" fill="{color}"/>')

    def icon(self, name, x, y, size, color):
        source = (ROOT / 'assets/icons' / f'{name}.svg').read_text(encoding='utf-8')
        body = re.search(r'<svg[^>]*>(.*)</svg>', source, re.S).group(1)
        self.parts.append(f'<g transform="translate({x} {y}) scale({size / 24})" stroke="{color}" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" fill="none">{body}</g>')

    def cat(self, kind, x, y, size):
        self.parts.append(f'<g data-cat="{kind}" transform="translate({x} {y}) scale({size / 96})">{logo(kind)}</g>')

    def pill(self, x, y, width, label, p, tone='muted', height=22):
        color, fill = {'muted': (p['muted'], p['bg']), 'good': (p['good'], p['goodbg']),
                       'warn': (p['warn'], p['warnbg']), 'accent': (p['accent'], p['soft'])}[tone]
        self.rect(x, y, width, height, fill, 6)
        self.text(x + width / 2, y + height / 2 + 3.5, label, 10.5, color, 600, 'middle')

    def button(self, x, y, width, label, p, primary=False, icon=None, height=30):
        fill = p['accent'] if primary else p['surface']
        ink = p['bg'] if primary and p is PALETTES['c'] else '#ffffff' if primary else p['ink']
        self.rect(x, y, width, height, fill, 6, None if primary else p['line'])
        if icon:
            self.icon(icon, x + 11, y + (height - 14) / 2, 14, ink)
            self.text(x + 33, y + height / 2 + 4, label, 11.5, ink, 600)
        else:
            self.text(x + width / 2, y + height / 2 + 4, label, 11.5, ink, 600, 'middle')

    def card(self, x, y, width, height, p, radius=8):
        self.rect(x, y, width, height, p['surface'], radius, p['line'])

    def image(self, child, x, y, width):
        height = width * child.height / child.width
        self.parts.append(f'<svg x="{x}" y="{y}" width="{width}" height="{height}" viewBox="0 0 {child.width} {child.height}">{"".join(child.parts)}</svg>')

    def save(self, name):
        text = f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.width}" height="{self.height}" viewBox="0 0 {self.width} {self.height}">\n' + '\n'.join(self.parts) + '\n</svg>\n'
        (HERE / name).write_text(text, encoding='utf-8')


def frame(s, p):
    s.parts.append('<defs><clipPath id="window"><rect width="1080" height="680" rx="10"/></clipPath></defs><g clip-path="url(#window)">')
    s.rect(0, 0, 1080, 680, p['bg'])
    s.rect(0, 0, 1080, 32, p['caption'])
    s.text(16, 21, 'mogumogu', 11, p['muted'], 600)
    s.line(952, 16, 964, 16, p['muted'])
    s.rect(997, 11, 10, 10, 'none', 0, p['muted'])
    s.line(1046, 11, 1056, 21, p['muted'])
    s.line(1056, 11, 1046, 21, p['muted'])


def finish(s, p, left=198):
    s.rect(0, 656, 1080, 24, p['caption'])
    s.line(0, 656, 1080, 656, p['line'])
    s.circle(left, 668, 3, p['good'])
    s.text(left + 12, 672, 'Bereit', 10.5, p['muted'])
    s.text(1060, 672, 'Demo · Beispielbestand', 10.5, p['muted'], anchor='end')
    s.parts.append('</g>')
    s.rect(0.5, 0.5, 1079, 679, 'none', 10, p['line'])


def search(s, x, y, width, p):
    s.rect(x, y, width, 30, p['bg'], 6, p['line'])
    s.icon('search', x + 10, y + 8, 14, p['muted'])
    s.text(x + 32, y + 20, 'Projekte suchen', 11.5, p['muted'])
    s.text(x + width - 10, y + 20, 'Strg+F', 10, p['muted'], anchor='end')


def cloud():
    s, p = Svg(), PALETTES['a']
    frame(s, p)
    s.rect(0, 32, 178, 624, p['nav'])
    s.cat('a', 17, 46, 34)
    s.text(60, 70, 'mogumogu', 18, p['ink'], 700)
    for i, (name, icon) in enumerate(NAV):
        y = 106 + i * 38 if i < 7 else 592
        if i == 0:
            s.rect(10, y, 158, 34, p['surface'], 7)
            s.rect(10, y + 9, 3, 16, p['accent'], 1.5)
        s.icon(icon, 23, y + 9, 16, p['accent'] if i == 0 else p['muted'])
        s.text(49, y + 22, name, 12, p['ink'] if i == 0 else p['muted'], 600 if i == 0 else 400)
        if i == 5:
            s.pill(139, y + 7, 22, '3', p, 'warn', 20)
    s.rect(178, 32, 902, 56, p['surface'])
    s.line(178, 88, 1080, 88, p['line'])
    s.text(198, 66, 'Arbeitsplatz', 12, p['ink'], 600)
    s.text(278, 66, '/ Übersicht', 12, p['muted'])
    search(s, 676, 45, 246, p)
    s.button(934, 45, 126, 'Aktualisieren', p, icon='refresh')
    s.text(198, 122, 'Übersicht', 22, p['ink'], 700)
    s.text(198, 143, 'Projekte, Paketbelege und nächste Prüfungen.', 11.5, p['muted'])
    s.button(752, 109, 156, 'Projekt registrieren', p, icon='plus')
    s.button(920, 109, 140, 'Freigaben erfassen', p, primary=True)
    for i, (title, value, icon, note) in enumerate([
        ('Projekte', '6', 'folder', 'registriert'), ('Paketversionen', '428', 'package', 'im Beispielbestand'),
        ('Prüfhinweise', '3', 'leaf', 'Entscheidung offen'), ('Erfasster Umfang', '142,0 GB', 'database', 'Beispielbestand')]):
        x = 198 + i * 218
        s.card(x, 165, 208, 68, p)
        s.icon(icon, x + 13, 178, 15, p['warn'] if i == 2 else p['accent'])
        s.text(x + 37, 190, title, 11, p['muted'])
        s.text(x + 13, 218, value, 22, p['ink'], 700)
        s.text(x + (115 if i == 3 else 63 if i == 1 else 42), 216, note, 10, p['muted'])
    s.card(198, 249, 578, 343, p)
    s.text(216, 276, 'Projekte', 14, p['ink'], 600)
    s.pill(500, 260, 64, 'Alle 6', p, 'accent')
    s.pill(572, 260, 80, 'Belegt 3', p)
    s.pill(660, 260, 96, 'Prüfen 1', p)
    for x, value in [(216, 'PROJEKT'), (384, 'PFAD'), (586, 'GRÖSSE'), (674, 'BEOBACHTUNG')]:
        s.text(x, 310, value, 9, p['muted'], 600)
    for i, (name, path, size, state, _) in enumerate(PROJECTS):
        y = 325 + i * 40
        if i == 2:
            s.rect(206, y, 562, 39, p['soft'], 5)
            s.rect(206, y + 9, 3, 21, p['accent'], 1.5)
        if i != 5:
            s.line(216, y + 40, 756, y + 40, p['line'])
        s.icon('folder', 216, y + 13, 14, p['accent'])
        s.text(238, y + 25, name, 11.5, p['ink'], 600 if i == 2 else 400)
        s.text(384, y + 25, path, 10.5, p['muted'])
        s.text(636, y + 25, size, 11, p['ink'], anchor='end')
        s.pill(674, y + 9, 80, state, p, 'good' if state == 'Belegt' else 'warn' if state == 'Prüfen' else 'muted')
    s.text(216, 581, 'Alle Projekte öffnen  →', 10.5, p['accent'], 600)
    s.card(790, 249, 270, 196, p)
    s.text(808, 272, 'AUSGEWÄHLT', 9.5, p['muted'], 600)
    s.icon('folder', 808, 287, 20, p['accent'])
    s.text(839, 303, 'ml-experiments', 15, p['ink'], 600)
    s.text(808, 326, r'C:\dev\ml', 11, p['muted'])
    s.line(808, 340, 1042, 340, p['line'])
    s.text(808, 364, 'Beobachtung', 11, p['muted'])
    s.pill(966, 348, 76, 'Prüfen', p, 'warn')
    s.icon('lock', 808, 380, 13, p['good'])
    s.text(830, 391, 'Geschützt registriert', 11, p['good'])
    s.button(808, 405, 234, 'Details öffnen', p)
    s.card(790, 457, 270, 135, p)
    s.text(808, 482, 'Speicherüberblick', 12, p['ink'], 600)
    for i, (name, amount, color, width) in enumerate([
        ('Projektquellen', '52 GB', '#7298e0', 136), ('Paket-Caches', '38 GB', '#8fb6c8', 100),
        ('Weitere Ressourcen', '52 GB', '#8cae9c', 136)]):
        y = 502 + i * 30
        s.text(808, y, name, 10.5, p['muted'])
        s.text(1042, y, amount, 10.5, p['ink'], anchor='end')
        s.rect(808, y + 6, 234, 3, p['bg'], 1.5)
        s.rect(808, y + 6, width, 3, color, 1.5)
    s.rect(198, 607, 862, 32, p['surface'], 6)
    s.icon('activity', 210, 615, 14, p['muted'])
    s.text(232, 628, 'Letzte Aktivität', 10.5, p['muted'], 600)
    s.text(330, 628, 'Beispielinventar geladen', 11, p['ink'])
    s.text(1046, 628, 'Aktivität öffnen  →', 10.5, p['accent'], anchor='end')
    finish(s, p)
    return s


def moss():
    s, p = Svg(), PALETTES['b']
    frame(s, p)
    s.rect(0, 32, 1080, 98, p['caption'])
    s.cat('b', 22, 43, 38)
    s.text(71, 69, 'mogumogu', 19, p['ink'], 700)
    s.text(190, 68, 'Lokales Inventar', 11.5, p['muted'])
    search(s, 616, 47, 254, p)
    s.button(886, 47, 174, 'Freigaben erfassen', p, primary=True, icon='search')
    x = 24
    for i, ((name, _), width) in enumerate(zip(NAV, [104, 92, 82, 144, 112, 100, 96, 120])):
        if i == 0:
            s.rect(x - 8, 92, width, 31, p['soft'], 6)
        s.text(x + 7, 113, name, 12, p['ink'] if i == 0 else p['muted'], 600 if i == 0 else 400)
        x += width
    s.line(20, 130, 1060, 130, p['line'])
    s.card(20, 150, 280, 488, p, 10)
    s.text(39, 181, 'Projekte', 16, p['ink'], 600)
    s.pill(114, 164, 25, '6', p)
    s.button(251, 162, 30, '+', p)
    s.text(39, 205, 'Dein registrierter Arbeitsplatz', 11, p['muted'])
    for i, (name, _, _, state, eco) in enumerate(PROJECTS):
        y = 222 + i * 55
        if i == 2:
            s.rect(30, y, 260, 50, p['soft'], 7)
            s.rect(30, y + 13, 3, 24, p['accent'], 1.5)
        s.icon('folder', 42, y + 15, 18, p['accent'] if i == 2 else p['muted'])
        s.text(72, y + 21, name, 12, p['ink'], 600 if i == 2 else 400)
        s.text(72, y + 39, eco, 10.5, p['muted'])
        s.circle(274, y + 25, 3, p['warn'] if state == 'Prüfen' else p['good'] if state == 'Belegt' else p['line'])
    s.rect(32, 574, 256, 52, p['nav'], 7)
    s.icon('leaf', 44, 591, 17, p['accent'])
    s.text(72, 596, '3 Prüfhinweise', 11.5, p['ink'], 600)
    s.text(72, 614, 'Zur Prüfliste  →', 10.5, p['accent'])
    s.text(320, 181, 'ml-experiments', 24, p['ink'], 700)
    s.text(320, 205, r'C:\dev\ml', 12, p['muted'])
    s.pill(734, 164, 75, 'Prüfen', p, 'warn')
    s.pill(819, 164, 102, 'Geschützt', p, 'good')
    s.button(935, 163, 125, 'Details öffnen', p)
    s.card(320, 235, 740, 68, p, 8)
    for i, (label, value) in enumerate([('Erfasster Umfang', '25,4 GB'), ('Ökosystem', 'Python · uv'), ('Beobachtung', 'Prüfen')]):
        x = 340 + i * 246
        s.text(x, 258, label, 10.5, p['muted'])
        s.text(x, 286, value, 18, p['ink'] if i < 2 else p['warn'], 600)
        if i < 2:
            s.line(x + 222, 250, x + 222, 289, p['line'])
    s.text(336, 337, 'Bestand', 12, p['accent'], 600)
    s.text(429, 337, 'Aktivität', 12, p['muted'])
    s.line(320, 349, 1060, 349, p['line'])
    s.line(320, 349, 400, 349, p['accent'], 2)
    s.card(320, 368, 480, 216, p, 10)
    s.text(340, 396, 'Pakete in der Umgebung', 14, p['ink'], 600)
    s.text(340, 418, 'Statischer Beispielbestand', 10.5, p['muted'])
    for i, (name, version) in enumerate([('numpy', '2.1.0'), ('pandas', '2.2.3'), ('requests', '2.32.3'), ('scikit-learn', '1.5.2')]):
        y = 436 + i * 34
        s.icon('package', 340, y + 6, 14, p['accent'])
        s.text(363, y + 18, name, 11.5, p['ink'])
        s.text(544, y + 18, version, 11, p['muted'])
        s.text(779, y + 18, 'Quelle nicht geprüft', 10.5, p['muted'], anchor='end')
        if i < 3:
            s.line(340, y + 29, 779, y + 29, p['line'])
    s.rect(818, 368, 242, 216, '#f1e9db', 10)
    s.icon('leaf', 837, 388, 22, p['warn'])
    s.text(837, 439, 'Prüfung offen', 16, p['ink'], 600)
    s.text(837, 465, 'Zweck und Besitzer dieser', 11.5, p['muted'])
    s.text(837, 483, 'Umgebung prüfen. Fehlende', 11.5, p['muted'])
    s.text(837, 501, 'Nutzung bleibt unbekannt.', 11.5, p['muted'])
    s.button(837, 531, 204, 'Belege ansehen', p)
    s.rect(320, 604, 740, 34, p['surface'], 7)
    s.icon('activity', 332, 613, 15, p['muted'])
    s.text(357, 626, 'Beispielinventar geladen', 11, p['muted'])
    s.text(1044, 626, 'Aktivität öffnen  →', 10.5, p['accent'], anchor='end')
    finish(s, p, 22)
    return s


def midnight():
    s, p = Svg(), PALETTES['c']
    frame(s, p)
    s.rect(0, 32, 64, 624, p['nav'])
    s.cat('c', 13, 45, 38)
    for i, (_, icon) in enumerate(NAV):
        y = 109 + i * 46 if i < 7 else 600
        if i == 0:
            s.rect(11, y, 42, 36, p['soft'], 8)
            s.rect(1, y + 10, 2, 16, p['accent'], 1)
        s.icon(icon, 23, y + 10, 18, p['accent'] if i == 0 else p['muted'])
        if i == 5:
            s.circle(47, y + 8, 3, p['warn'])
    s.text(84, 65, 'mogumogu', 13, p['ink'], 600)
    s.text(170, 65, '/ Übersicht', 12, p['muted'])
    search(s, 704, 45, 258, p)
    s.icon('refresh', 990, 52, 17, p['muted'])
    s.icon('dots', 1032, 52, 17, p['muted'])
    s.text(84, 112, 'Prüfübersicht', 23, p['ink'], 600)
    s.text(1060, 110, 'Belege zuerst. Entscheidungen mit Kontext.', 11, p['muted'], anchor='end')
    s.rect(84, 134, 976, 60, p['surface'], 7)
    for i, (label, number, icon) in enumerate([('Projekte', '6', 'folder'), ('Paketversionen', '428', 'package'), ('Prüfhinweise', '3', 'leaf'), ('Beispielbestand', '142,0 GB', 'database')]):
        x = 100 + i * 244
        s.icon(icon, x, 156, 17, p['warn'] if i == 2 else p['muted'])
        s.text(x + 30, 173, number, 22, p['ink'], 600)
        s.text(x + (137 if i == 3 else 88 if i == 1 else 57), 172, label, 11, p['muted'])
    s.card(84, 213, 190, 400, p, 7)
    s.text(100, 240, 'Projekte', 12, p['ink'], 600)
    s.text(256, 240, '6', 11, p['muted'], anchor='end')
    for i, (name, _, _, _, eco) in enumerate(PROJECTS):
        y = 259 + i * 50
        if i == 2:
            s.rect(92, y, 174, 46, p['soft'], 5)
        s.icon('folder', 101, y + 8, 14, p['accent'] if i == 2 else p['muted'])
        s.text(124, y + 20, name, 11.5, p['ink'], 600 if i == 2 else 400)
        s.text(124, y + 37, eco, 10, p['muted'])
    s.card(290, 213, 502, 400, p, 7)
    s.text(308, 241, 'Prüfliste', 15, p['ink'], 600)
    s.pill(744, 225, 28, '3', p, 'warn')
    s.text(308, 264, 'Hinweise brauchen eine Entscheidung.', 11, p['muted'])
    for i, (name, subtitle) in enumerate([
        ('ml-experiments', 'Umgebung · Prüfung offen'),
        ('notebook-spike', 'Scratchpad · Prüftermin erreicht'),
        ('build-cache', 'Cache · Zuordnung prüfen')]):
        y = 281 + i * 64
        if i == 0:
            s.rect(300, y, 482, 58, p['soft'], 5)
        s.icon('leaf' if i < 2 else 'database', 311, y + 18, 17, p['warn'])
        s.text(341, y + 24, name, 12, p['ink'], 600)
        s.text(341, y + 43, subtitle, 10.5, p['muted'])
        s.pill(700, y + 17, 68, 'Prüfen', p, 'warn')
    s.line(308, 484, 774, 484, p['line'])
    s.text(308, 508, 'Letzte Aktivität', 11.5, p['ink'], 600)
    for i, text in enumerate(['Beispielinventar geladen', 'Session im Beispiel abgeschlossen', 'Paketstand im Beispiel aktualisiert']):
        y = 532 + i * 27
        s.circle(314, y - 4, 2.5, p['muted'])
        s.text(328, y, text, 10.5, p['muted'])
    s.card(808, 213, 252, 400, p, 7)
    s.text(826, 239, 'KONTEXT', 9.5, p['muted'], 600)
    s.icon('folder', 826, 258, 20, p['accent'])
    s.text(856, 274, 'ml-experiments', 14.5, p['ink'], 600)
    s.text(826, 299, r'C:\dev\ml', 11, p['muted'])
    s.text(826, 344, '25,4 GB', 25, p['ink'], 600)
    s.text(826, 365, 'erfasst · Beispielbestand', 10.5, p['muted'])
    s.line(826, 381, 1042, 381, p['line'])
    s.text(826, 406, 'Beobachtung', 10.5, p['muted'])
    s.pill(966, 390, 76, 'Prüfen', p, 'warn')
    s.icon('lock', 826, 426, 14, p['good'])
    s.text(849, 438, 'Geschützt registriert', 11, p['good'])
    s.rect(826, 459, 216, 78, p['bg'], 6)
    s.text(839, 482, 'Nutzung bleibt unbekannt.', 11, p['ink'], 600)
    s.text(839, 504, 'Fehlende Verweise sind', 10.5, p['muted'])
    s.text(839, 521, 'kein Löschbeleg.', 10.5, p['muted'])
    s.button(826, 559, 216, 'Details öffnen', p)
    s.button(84, 624, 163, 'Freigaben erfassen', p, primary=True)
    s.button(259, 624, 163, 'Projekt registrieren', p, icon='plus')
    s.text(1060, 644, 'Strg+F  Suche     ·     F5  Ansicht aktualisieren', 10, p['muted'], anchor='end')
    finish(s, p, 84)
    return s


def logo_sheet():
    s = Svg(1440, 540)
    s.rect(0, 0, 1440, 540, '#f5f7fb')
    s.text(40, 48, 'Same little cat. Three personalities.', 26, '#223650', 600)
    s.text(40, 76, 'Editable SVG marks · transparent backgrounds · large and small-size previews', 13, '#63748b')
    for i, (kind, title, subtitle) in enumerate([
        ('a', 'A · Mochi', 'Round, curious, closest to the current mark'),
        ('b', 'B · Nibble', 'Chewing cheeks and a tiny raised paw'),
        ('c', 'C · Pocket', 'A little cat peeking over two soft paws')]):
        x = 40 + i * 466
        s.rect(x, 104, 428, 398, '#ffffff', 14, '#dce4ef')
        s.cat(kind, x + 119, 120, 190)
        s.text(x + 214, 336, title, 20, '#223650', 600, 'middle')
        s.text(x + 214, 362, subtitle, 11.5, '#63748b', anchor='middle')
        s.rect(x + 28, 390, 180, 84, '#edf2f9', 10)
        s.rect(x + 220, 390, 180, 84, '#172436', 10)
        for start in [x + 42, x + 234]:
            for j, size in enumerate([32, 24, 16]):
                xx = start + j * 53
                s.cat(kind, xx, 408 + (32 - size) / 2, size)
                s.text(xx + size / 2, 459, size, 9.5, '#63748b' if start == x + 42 else '#a1b1c8', anchor='middle')
    s.save('logos.svg')


def board(screens):
    s = Svg(1760, 1010)
    s.rect(0, 0, 1760, 1010, '#f3f5f9')
    s.text(44, 56, 'mogumogu · design variants', 30, '#223650', 600)
    s.text(44, 87, 'Compact Windows layouts, three cat personalities, one sample inventory.', 15, '#63748b')
    for i, (key, name, description, logo_name, file) in enumerate([
        ('a', 'Cloud', 'Overview first · familiar navigation', 'Mochi', 'a-cloud'),
        ('b', 'Moss', 'Project workspace · warm and quiet', 'Nibble', 'b-moss'),
        ('c', 'Midnight', 'Review first · compact dark workspace', 'Pocket', 'c-midnight')]):
        x = 44 + i * 574
        s.rect(x, 118, 544, 840, '#ffffff', 14, '#dce4ef')
        s.text(x + 22, 157, key.upper() + ' / ' + name, 24, '#223650', 600)
        s.text(x + 22, 184, description, 12, '#63748b')
        s.image(screens[key], x + 16, 207, 512)
        s.cat(key, x + 184, 579, 176)
        s.text(x + 272, 797, 'mogumogu', 30, '#223650', 700, 'middle')
        s.text(x + 272, 830, logo_name, 15, '#63748b', anchor='middle')
        palette = PALETTES[key]
        for j, color in enumerate([palette['bg'], palette['surface'], palette['soft'], palette['accent'], palette['ink']]):
            s.rect(x + 180 + j * 38, 863, 28, 28, color, 7, '#dce4ef')
        s.text(x + 272, 925, '1080 × 680 · vector logo included', 11.5, '#63748b', anchor='middle')
    s.text(44, 989, 'Visual design concepts · synthetic data · preview files in design/variants/', 12, '#63748b')
    s.save('comparison.svg')


def gallery(screens):
    """A self-contained comparison viewer; the app drawings are read-only."""
    template = (HERE / 'gallery.html').read_text(encoding='utf-8')
    bodies = []
    for key in screens:
        screen = screens[key]
        content = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1080 680" role="img" aria-label="Windows application design concept">{"".join(screen.parts)}</svg>'
        bodies.append(f'<template id="screen-{key}">{content}</template>')
    template = template.replace('<!-- SCREENS -->', '\n'.join(bodies))
    template = template.replace('/* MARKS */ {}', json.dumps({key: logo(key) for key in screens}))
    (HERE / 'index.html').write_text(template, encoding='utf-8')


if __name__ == '__main__':
    screens = {'a': cloud(), 'b': moss(), 'c': midnight()}
    for kind, name in [('a', 'a-cloud'), ('b', 'b-moss'), ('c', 'c-midnight')]:
        screens[kind].save(name + '.svg')
    for kind, name in [('a', 'mochi'), ('b', 'nibble'), ('c', 'pocket')]:
        s = Svg(96, 96)
        s.parts.append(logo(kind))
        s.save(f'logo-{kind}-{name}.svg')
    logo_sheet()
    board(screens)
    gallery(screens)
    print('Generated three UI concepts, three transparent vector marks and two comparison sheets.')
