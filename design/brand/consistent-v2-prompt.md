# Mochi: vereinheitlichter Zeichensatz v2

Erstellt am 18. Oktober 2026 mit dem eingebauten `image_gen`-Werkzeug.
Der Generator bietet keine explizite Modellwahl und meldet keine Modellkennung.
Ausgabe unverändert nach `assets/brand/mascots-v2.png` kopiert; Generatororiginal
beibehalten. Ein gemeinsamer 2×2-Bogen sorgt für denselben Zeichenmaßstab.

Eingaben (Edit-Ziele, in dieser Reihenfolge):

1. `assets/brand/mascot-neutral.png`
2. `assets/brand/mascot-happy.png`
3. `assets/brand/mascot.png`
4. `assets/brand/mascot-focused.png`

## Vollständiger Prompt

```text
Use case: style-transfer, identity-preserve.
Asset type: ONE coordinated 2-by-2 character sprite sheet for the mogumogu native Windows app.
Edit targets: image 1 is Mochi sitting neutrally; image 2 is Mochi happy with raised little paws; image 3 is Mochi holding a blue folder; image 4 is Mochi concentrating behind a laptop. Harmonize these exact four poses into one consistent drawing set. Preserve this original cat's identity and the recognizable poses.

Output a square 1024 by 1024 image, strict equal 2-by-2 grid with four 512 by 512 cells, no visible grid or panels. Top left NEUTRAL, top right HAPPY, bottom left FOLDER, bottom right LAPTOP. Those words describe placement only; do NOT draw any text. Each pose stays fully inside its own cell. Keep the same head scale, head/body proportions, foot/ground baseline and generous even padding in all four cells. Each complete pose should fill about 86-90% of its cell height.

Make a restrained consistency pass that retains the soft original manga/sketchbook charm. Use the same single charcoal ink contour everywhere, approximately 6 px outer contour and 4 px small detail strokes per 512 px cell, with round ends and only a slight natural hand-drawn wobble. Eliminate doubled scratchy contours, different pencil weights and varying darkness between poses. Keep faint delicate texture inside the fills, with clean readable edges. Same oval head construction, short triangular cat ears, blue inner ears, three whiskers per side, small dot eyes, tiny w-shaped mouth and oval blush cheeks with three short hatch marks. The happy expression keeps its closed smiling eyes and small open happy mouth; do not make all expressions identical.

Identical palette across all four: warm cream-white fur, slate charcoal #4B5563 contours, pale sky blue #C7E0F6 ear interiors and folder, soft blush pink #FADADD cheeks. Same fur shading strength and same cheek size/colour throughout. Keep the blue folder, blue-grey laptop and their tiny white cat badge faithful to the references. Keep stubby paws, small feet and the existing left-side tails. Retain sparse little manga expression ticks where present, at the same thin stroke weight.

Background: opaque uniform warm paper white #FFFDF8 across the entire sheet, with only an extremely faint even paper texture. Give every pose the same small pale-blue ground shadow. Do not add a coloured tile behind any pose.
No headings, labels, captions, lettering, font samples, app interface, swatches, borders, extra characters, props, watermark or franchise figures. No glossy 3D, vector emoji, heavy black cartoon outlines or furry realism.
```
