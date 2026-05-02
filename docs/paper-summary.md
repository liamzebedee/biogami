# Nagpal 2001 — Programmable Self-Assembly

Summary notes from Radhika Nagpal's MIT PhD thesis,
*Programmable Self-Assembly: Constructing Global Shape using
Biologically-inspired Local Interactions and Origami Mathematics* (advisors:
Sussman, Abelson). These are the core ideas that drove this Rust
implementation.

## 1. Problem and approach

How do you program millions of identical, unreliable, locally-communicating
cells to assemble themselves into a *specific* global shape?

Nagpal's answer is a two-level system:

- **Global level**: describe the desired shape as a folding construction on a
  flat continuous sheet, using a small Lisp-flavoured DSL — the **Origami
  Shape Language (OSL)** — built around Huzita's axioms of paper folding.
- **Local level**: a compiler turns that global description into a single
  **cell program** that every cell runs identically. Cells coordinate using
  a tiny set of biologically-inspired primitives (gradients, neighborhood
  query, polarity inversion, cell-to-cell contact, flexible folding).

The thesis demonstrates the language can synthesize flat layered shapes (cup,
airplane, samurai hat, envelope), all plane Euclidean constructions, and a
variety of tessellation patterns, robustly, on randomly distributed cells with
no global clock and no global coordinates.

## 2. The cell sheet model

A single layer of identically-programmed flexible cells fills a square
sheet. The model used in chapters 3–7:

- Cells are **randomly but densely distributed** in the sheet (typical
  simulation: 1000–8000 cells).
- Each cell talks only to neighbors within a fixed communication radius `r`
  (typical neighborhood: 15–20 cells).
- Cells have an **apical** and **basal** surface (top/bottom). All cells
  start with the same polarity.
- Each cell has a small amount of local boolean state and can generate
  *probably-locally-unique* identifiers from a PRNG.
- Cells are **asynchronous** — no global clock; no shared coordinates.
- Cells can sense direct contact between their apical/basal surface and
  another cell's apical/basal surface (this is how folded layers re-merge).
- Cells can virtually contract fibers along a locally-determined orientation
  — `local-fold(direction, surface)` — and the simulator turns a population
  of locally agreeing cells into a flat sheet fold.

Initial conditions known to every cell: which of the four edges (`e12`,
`e23`, `e34`, `e41`) and corners (`c1`–`c4`) it belongs to (each is just a
boolean state variable; cells don't know *where* in an edge they are).

## 3. Huzita's axioms (subset used)

OSL uses Huzita's first four axioms — sufficient for plane Euclidean
constructions:

| Axiom | OSL primitive          | What it constructs                                 |
|-------|------------------------|----------------------------------------------------|
| A1    | `(crease-lbp p1 p2)`   | Line through two points                            |
| A2    | `(crease-p2p p1 p2)`   | Perpendicular bisector of segment p1–p2            |
| A3    | `(crease-l2l l1 l2)`   | Angle bisector of two lines (or midline if ∥)      |
| A4    | `(crease-l2s l1 p1)`   | Through `p1`, perpendicular to `l1`                |

Each axiom defines a *crease* — a line on the sheet — but does not by itself
fold anything. Folds are commanded explicitly by `execute-fold`.

## 4. Origami Shape Language — surface

Programs are sequences of operations on points, lines, and regions.

```
(define name expr)             ; bind a name
(defun (name args...) body...) ; user procedures (used for repeated structure)
```

Primitive operations:

- `crease-lbp / crease-p2p / crease-l2l / crease-l2s` — Huzita axioms (return a line)
- `(intersect l1 l2)` — return the intersection point
- `(execute-fold line type landmark=p)` — fold the sheet along `line`. `type`
  is `apical` or `basal` (valley/mountain relative to current top surface).
  The `landmark` point/line picks which side is the *moving* side: cells on
  the opposite side reflect across `line` and invert their polarity.
- `(create-region p1 l1)` — return a region: the side of `l1` that contains
  `p1` (a half-plane).
- `(within-region r expr...)` — restrict the enclosed operations to cells in
  region `r`. Crucial for partial-layer folds (folding a flap, not the whole
  stack).
- `(or ...)`, `(not ...)` — region combinators.

Sheet starts square with edges/corners pre-bound. The cup, airplane, and
samurai-hat programs in Figs 3-4, 3-5, 3-6 all use only these primitives.

## 5. Biologically-inspired local primitives

The cell-level building blocks used to implement each OSL operation
(chapter 4):

1. **Gradients** — `(create-gradient g)` floods a name `g` outward through
   the neighbor graph, value incrementing by one each hop. Cells store the
   minimum value heard. Effectively a spatial BFS that approximates Euclidean
   distance to the source. `(recvd? g)` and gradient-value lookups let cells
   decide whether they're equidistant to two sources (axiom 2/3) or pick a
   local "growing point" (axiom 1).
2. **Neighborhood query** — broadcast a question to neighbors and collect
   their answers (e.g., gradient values, polarity).
3. **Polarity inversion** — flip the cell's apical/basal orientation. Used
   only after a fold to keep the sheet's surfaces consistent.
4. **Cell-to-cell contact** — folded layers physically touch and become
   communication neighbors, so gradients seep through stacked layers (this
   is how the cell program treats a folded sheet as a single layer for the
   purpose of further folding).
5. **Flexible folding** — `local-fold(direction, surface)` is the cell-level
   actuation; the simulator turns the union of locally-folding cells into a
   global flat fold along the best-fit line.

## 6. Implementing OSL operations as cell programs

Each OSL operation becomes a small Scheme-style cell-level procedure (the
`-rule` family) using only the five primitives above.

- **Axiom 2/3 (`crease-p2p`, `crease-l2l`)** — `axiom2-rule`: cells in `p1`
  emit gradient `g1`. When `g1` reaches `p2`, those cells emit `g2`. When
  both arrive everywhere, cells where `|g1 − g2| < ~2r` belong to the
  bisector crease. `p1` then emits a third "completion" gradient `gend`.
- **Axiom 1/4 (`crease-lbp`, `crease-l2s`)** — `axiom1-rule`: borrowed from
  Coore's Growing Point Language. `p2` emits a gradient. The `p1` cell with
  the lowest gradient value becomes the "growing point" and passes a token
  along the steepest descent toward `p2`, leaving a trail of crease cells
  along the way.
- **Intersect (`intersect`)** — `intersect-rule`: trivially `l1 AND l2` per
  cell. Cells where both line-vars are true form the point.
- **Create-region** — `create-region-rule`: cells in `p1` emit a *bounded*
  gradient that cells of `l1` refuse to forward. Cells that hear the
  gradient are in the region.
- **Within-region** — every cell tracks `current-region`. If false the cell
  becomes a no-op for the duration of the block (still forwards messages).
- **Execute-fold** — `execute-fold-rule`: landmark cells create a bounded
  region (so they know which side they're on); crease cells locally estimate
  the fold orientation by measuring how many of their neighbors are also in
  the crease, then call `local-fold`. The simulator computes the best-fit
  line and applies it. Marked cells flip polarity afterwards.

Each operation ends with a final completion gradient — the natural
**barrier synchronization** that makes asynchronous cells safely sequence
between operations.

## 7. Compilation strategy (global → local)

Compiling an OSL program (chapter 4.3) is a straight syntax-directed pass:

1. For each `define name op`, allocate a boolean local state variable
   `name`.
2. Translate each call to its `*-rule` cell procedure, allocating fresh
   gradient names per operation. Three gradients per axiom suffice; names
   can be **flushed and reused** after a successful `execute-fold`, since
   folds destroy the spatial meaning of older gradients.
3. `within-region` becomes `(set! current-region <region>)` then a
   `(set! current-region #t)` at the end.
4. The first instruction of every cell program is a one-time *calibration*
   that bounces a gradient `c1 ↔ c3` to estimate cross-sheet round-trip
   time. This `local-delay` is later used to wait between phases of an
   axiom so cells don't sample a half-propagated gradient.

Result: the cell program is mostly **identical across all shapes** — the
fixed plumbing for the five primitives plus the `*-rule` library. Only the
~tens of lines that mirror the OSL ops vary. Nagpal draws the analogy to
DNA: most of it is housekeeping; only a small fraction encodes the specific
shape.

The cup-program example (Fig 4-8) compiles to exactly the structure this
implementation emits in `bgcompile`:

```scheme
(set! d1   (axiom2-rule c3 c1 g1 g2 g3))
(set! front (create-region-rule c3 d1 g4 g5))
(set! back  (create-region-rule c1 d1 g6 g7))
(execute-fold-rule d1 apical c3 g8 g9)
...
```

## 8. Robustness — why it works at all (chapter 6)

- A neighborhood of ~15 cells is enough for gradient values to estimate
  distance to within roughly one hop, after smoothing with neighbor
  averaging.
- Crease width is set to about `2r` (≈ 2 average inter-cell distances),
  giving redundancy against gradient noise. Below ~5 cells of width, axiom
  reliability degrades sharply.
- Two interfering gradients (e.g., when many axioms run in parallel) blur
  the equidistant locus by a fraction of `r`; this puts a soft lower bound
  on how thin a region can reliably be.
- Random cell death up to a few percent is tolerated because gradients
  re-route through alternate neighbors; isolated dead cells become invisible
  rather than catastrophic.

<!-- ## 9. What this implementation faithfully reproduces

- OSL surface syntax: all primitives, `define`, `defun`, regions, keyword
  `landmark=` argument.
- Huzita axioms 1–4 exactly (analytic, in `geom.rs`); plus a layered Sheet
  that folds by polygon clipping + reflection.
- The compiler emits the same `axiomN-rule / intersect-rule / create-
  region-rule / execute-fold-rule` form with allocated gradient names.
- A cell-level simulator with randomly distributed cells, BFS gradient
  propagation, bounded gradients for regions, `axiom2-rule`-style bisector
  selection, fold execution by best-fit line + reflection, and per-cell
  polarity flipping.
- Side-by-side 2D and animated 3D views of the cell sheet, gradient field,
  and folds. -->

## 10. What this implementation simplifies or skips

- The cell-level **calibration phase** is implicit (the simulator uses a
  global `r` and BFS hop count rather than a per-cell measured `local-
  delay`).
- **Asynchrony** is approximated: phases within an axiom are sequenced
  globally (g1, then g2, then crease selection) instead of cells using
  message quiescence to detect a stable gradient field.
- **Cell-to-cell contact** between folded layers is not yet modelled — the
  cell simulator's neighbor graph is fixed at construction. Folded sheets
  in our visualisation stack visually but don't fuse for gradient purposes.
- Axioms 5 and 6, and their non-Euclidean constructions (cube-doubling,
  trisection), are not implemented. The thesis only uses 1–4 anyway.
- Robustness analysis (chapter 6) — error rates, neighborhood sweeps,
  random cell death — is not reproduced.
- Tessellations and the D'Arcy-Thompson-style scale-independence
  experiments (chapter 7) are not yet implemented but the language and
  compiler should support them with no further work.

## References

- Nagpal, R. *Programmable Self-Assembly*. MIT PhD thesis, 2001.
- Huzita, H. (1989). The six axioms of origami.
- Coore, D. *Botanical Computing*. MIT PhD thesis, 1999 (Growing Point
  Language; cited heavily for axiom 1).
- Abelson et al. (1999). *Amorphous Computing* — the broader research
  programme this work fits inside.
