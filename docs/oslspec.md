# OSL: a sketch of the language and runtime

OSL — the **Origami Shape Language** — is the small DSL at the heart of
Radhika Nagpal's 2001 MIT thesis, *Programmable Self-Assembly: Constructing
Global Shape using Biologically-inspired Local Interactions and Origami
Mathematics*. biogami is a clean-room Rust reproduction of it.

This document is an informal walkthrough: what the language looks like, what
it means, and how a global geometric program ends up running on a sheet of
identical cells that only talk to their neighbours.

## The exercise

Imagine a sheet of paper made of a million tiny "cells", each one a small
agent with no global coordinates, no clock, no addressing. Every cell runs
the same program. They can only:

- read their own local state (a bag of boolean flags and scalar values),
- send and receive short messages to neighbours within some communication
  radius, and
- physically fold along lines that emerge from that local computation.

The thesis poses the question: what global shapes can such a sheet
fold itself into? And what would a programming model look like, given that
you'd never want to write the cell-level program by hand?

OSL is the answer to the second question. The runtime and compiler are the
machinery that connect the two.

## The language

OSL is a Lisp-flavoured s-expression language. There are only a handful of
forms, organised around the four classical Huzita axioms of origami
construction.

```
(crease-lbp p1 p2)        ; A1: line between two points
(crease-p2p p1 p2)        ; A2: perpendicular bisector of p1, p2
(crease-l2l l1 l2)        ; A3: angle bisector of two lines
(crease-l2s l1 p1)        ; A4: through p1, perpendicular to l1
(intersect l1 l2)         ; intersection point of two lines
(execute-fold line type landmark=p)  ; fold along `line`; type is apical|basal
(create-region p1 l1)     ; the half-plane of l1 that contains p1
(within-region r ...)     ; restrict ops to a region
(define name expr)        ; bind a name
(defun (name args...) body...)  ; procedure (inlined at call sites)
```

Built-in symbols name the four edges of the unit square — `e12 e23 e34 e41` —
and the four corners — `c1 c2 c3 c4`. Everything else is user-defined.

A canonical OSL program — the cup from Fig 3-4 of the thesis — looks like:

```lisp
(define d1 (crease-p2p c3 c1))
(define front (create-region c3 d1))
(define back  (create-region c1 d1))
(execute-fold d1 apical landmark=c3)

(define d2 (crease-l2l e23 d1))
(define p1 (intersect d2 e34))
(define d3 (crease-p2p c2 p1))
(execute-fold d3 apical landmark=c2)

(define p2 (intersect d3 e23))
(define d4 (crease-p2p c4 p2))
(execute-fold d4 apical landmark=c4)

(define l1 (crease-lbp p1 p2))
(within-region front (execute-fold l1 apical landmark=c3))
(within-region back  (execute-fold l1 basal  landmark=c1))
```

The whole thing reads as a global geometric construction — a recipe a
human folder could follow on a square of paper. Crucially, that recipe says
nothing about cells, gradients, or messages.

## Two ways to run a program

biogami has two interpreters for the same source, and the contrast between
them is the whole pedagogical point of the system.

### The global interpreter

`bgrun foo.osl` evaluates the program **analytically**. Lines are real
`Line` values, points are real `Vec2`s, axioms are evaluated as exact
geometric constructions, folds are reflections of polygon layers across a
line. This is the "ground truth" view: it tells you what shape a perfect
folder would produce.

This mode exists for two reasons. It's how you debug a new OSL program —
you want to know your construction is even geometrically valid before you
ask a million ant-sized agents to attempt it. And it's the oracle the
cell-level simulator is judged against.

### The compiler and cell runtime

`bgcompile foo.osl` lowers the same program to a **cell program** — also
an s-expression, but expressed in terms of primitive rules that a single
cell can actually run. The lowering is straightforward, one OSL form per
rule:

| OSL                          | Cell rule              |
|------------------------------|------------------------|
| `crease-lbp`, `crease-l2s`   | `axiom1-rule`          |
| `crease-p2p`, `crease-l2l`   | `axiom2-rule`          |
| `intersect`                  | `intersect-rule`       |
| `create-region`              | `create-region-rule`   |
| `execute-fold`               | `execute-fold-rule`    |
| `within-region` / end        | `set! current-region`  |
| `define`                     | local boolean state    |
| `defun`                      | inlined before lowering|

Each axiom rule is allocated a few fresh **gradient names** — these are
the scalar fields that will actually do the work at runtime.

The cell runtime, then, executes the lowered program on a sheet of
randomly-distributed cells. Each rule resolves to the same biologically-
plausible motif:

1. **Source** cells (those whose local state matches the rule's seed —
   e.g. cells tagged as corner `c1`) start broadcasting a gradient.
2. The gradient propagates by BFS over the local-radius neighbour graph.
   Distance values approximate Euclidean distance from the source.
3. Other cells **compare** gradient values to decide whether they sit on
   the line, on a particular side, in a region, etc. For example, an A2
   crease — perpendicular bisector of `p1` and `p2` — is the locus of
   cells where the gradients from `p1` and `p2` are equal.
4. A line, region, or landmark is just a boolean flag flipped on the cells
   that satisfy that comparison. There are no shared coordinates, no shared
   line equation; only a flag plus a stable identity.
5. `execute-fold-rule` is the one rule that breaks symmetry: cells on one
   side of the named line physically reflect across it, flip polarity, and
   bump their layer. Gradients are flushed and the next rule begins.

The crucial property is that the cell program is **the same** regardless of
how many cells, where they sit, or in what order they fire. The thesis's
contribution is that a small, fixed set of local rules — driven by a
handful of gradients — robustly reproduces the global construction.

## Regions and procedures

Two language features deserve a special mention because they're what make
non-trivial programs tractable.

**Regions.** `create-region` plus `within-region` lets you scope a block of
operations to a half-plane of the sheet. The cup's last two folds use this
to fold the front and back layers in opposite directions. At the cell
level, `within-region` compiles to nothing more than `(set! current-region
R)` before the block and `(set! current-region #t)` after — every rule
inside the block silently ANDs `current-region` into its participation
test.

**Procedures.** `defun` defines a parameterised construction (e.g.
`fold-wing` in `airplane.osl`). Procedures are **inlined** before
compilation — there is no cell-level call/return. The compiler enforces
this: `expand_defuns` runs first, substitutes arguments, and flattens the
result; the lowering pass never sees a `defun`. This keeps the cell
runtime stupid, which is the whole point.

## Where to look in the code

- `src/parser.rs`, `src/ast.rs` — the s-expression front end.
- `src/interp.rs`, `src/geom.rs`, `src/sheet.rs` — the global interpreter
  and analytical fold model.
- `src/compile.rs` — `expand_defuns` followed by `Compiler::compile` lowers
  OSL to the cell program.
- `src/cellsim.rs` — the cell sheet, neighbour graph, gradient BFS, and the
  rule implementations.
- `src/ui.rs` — the egui visualiser.
- `src/bin/bgcompile.rs`, `src/bin/bgrun.rs` — the two binaries.
- `examples/{cup,airplane,grid}.osl` — the three example programs that
  also drive the integration tests.

## Why the exercise matters

OSL is a study in a particular kind of compiler: one that takes a
*declarative geometric description* and lowers it to a *distributed,
local-rule program* that produces the same shape. Nothing about the source
program mentions cells, gradients, or messages — those are all
implementation. The fact that such a lowering is even possible, for a
non-trivial fragment of origami, is the surprise.

biogami exists to make that surprise tactile: write a few lines of Lisp,
watch a thousand dots converge on the same folded cup.
