The basic elements of the language are points, lines and regions. 

the sheet starts out with 
- four corner points (c1-c4) and 
- four edge lines (e12-e41) 

and an apical-basal polarity.


The axioms generate new lines from existing points and lines, while new points are created by intersecting lines

the first cup operation constructs the diagonal d1 from the points c1 and c2 by using axiom 2. 
The diagonal divides the sheet into two regions, front and back; 
a region is defined by a line that divides the sheet into two and a point on one side of the line. 

```sh
;; OSL Cup program (Nagpal thesis Fig 3-4)
;; A square sheet folded into a cup. Demonstrates regions and partial-layer folds.

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
(within-region front
  (execute-fold l1 apical landmark=c3))
(within-region back
  (execute-fold l1 basal landmark=c1))
```

Rather than specify a global shape directly, this language specifies a process for folding the shape. However this specification is abstract — the process is on a continuous sheet with no notion of agents or self-assembly


The agent programs are based on five primitives: 
    gradients, 
    neighborhood query, 
    polarity inversion, 
    cell-to-cell contact and 
    flexible folding. 
The primitives are simple ways
in which a agent interacts with its local neighborhood and
uses its sensing/actuation capabilities



Each
agent has a boolean variable in its internal state for each
distinct point/line; the variable is true if the agent is part of
the point/line. Initially the sheet starts out with four distinct lines and points (edges and corners), so all agents have
boolean state variables e12, e23, e34, e41, c1, c2, c3
c4. If an agent is part of the edge e12 then the corresponding state variable is true

The axioms use gradients to determine which agents belong to the crease. The axioms make use of the fact that
gradients provide a distance estimate as well as reflect the
shape of the source.


Axiom 1 uses tropism to create a crease from point p1 to
p2. Tropism implies the ability to sense the direction of a
gradient by comparing neighboring values. The agents in p2
create a gradient and the agents in p1 grow a crease towards
p2 by following decreasing gradient values

Axiom 2 creates a crease line such that any
point on the crease is equidistant from points p1 and p2.
Therefore if p1 and p2 generate two different gradients, each
agent can compare if the gradient levels are approximately
equal to determine if it is in the crease line.

Axiom 3 uses the same local rules as axiom 2.
It creates a crease
the gradients are produced by lines rather than points, and thus
the gradient values increase as one moves perpendicular to
the input line

Axiom 4 reuses the axiom 1 local rules to
grow a crease from point p1 to the crease l1.


A local rule for axiom 2.

```sh
(define (axiom2-rule p1 p2 g1 g2)
    ; arguments are boolean state p1 p2
    ; and gradient names g1 g2
    (if p1 (create-gradient g1))
    (if p2 (create-gradient g2))
    (wait-for-gradients g1 g2)
    
    ; generates a crease of width apprx 2r
    (if (< (abs (- g1 g2)) 1)
        #t
        #f))
```

`wait-for-gradients` is a form of messaging, like smalltalk. 
a gradient in this paper is implemented as a message broadcast to the neighbourhood
the value of that message is its hops from the sender
in a sense, the gradient is larger the further away the sender is
the gradient is like the inverse of how signals work normally - ie. it increases in value due to distance




The local rule returns a true or false value depending on whether the agent belongs to the
new point or crease and this true/false value can be stored
in some new local state variable. 

Each axiom attempts to produce creases that are approximately twice the width of
the local communication distance r.

#f means false, #t means true.

The compiled `cup.osl` - what each cell executes:

```sh
;; (base) liam@rand:~/Projects/biogami$ ./target/debug/bgcompile examples/cup.osl 
;; CELL PROGRAM
;; initial state: e12-e41, c1-c4, current-region=#t
(define d1 #f)
(define front #f)
(define back #f)
(define d2 #f)
(define p1 #f)
(define d3 #f)
(define p2 #f)
(define d4 #f)
(define l1 #f)
(set! local-delay (calibrate))
(set! d1 (axiom2-rule c3 c1 g1 g2 g3))
(set! front (create-region-rule c3 d1 g4 g5))
(set! back (create-region-rule c1 d1 g6 g7))
(execute-fold-rule d1 apical c3 g8 g9)
(set! d2 (axiom2-rule e23 d1 g10 g11 g12))
(set! p1 (intersect-rule d2 e34))
(set! d3 (axiom2-rule c2 p1 g13 g14 g15))
(execute-fold-rule d3 apical c2 g16 g17)
(set! p2 (intersect-rule d3 e23))
(set! d4 (axiom2-rule c4 p2 g18 g19 g20))
(execute-fold-rule d4 apical c4 g21 g22)
(set! l1 (axiom1-rule p1 p2 g23 g24))
(set! current-region front)
(execute-fold-rule l1 apical c3 g25 g26)
(set! current-region #t)
(set! current-region back)
(execute-fold-rule l1 basal c1 g27 g28)
(set! current-region #t)
```




Regions allow the user to restrict the context in which a
local rule applies. A region is created by using bounded
gradients, which implies that certain types of cells will not
forward the gradient message; the intuition being that certain cells can act as barriers to particular gradients. 

The agents in the point p1 create a bounded gradient that can
not pass through the agents in the dividing line l1. The
agents that receive the bounded gradient become part of
the region. Hence we can use gradients to mark regions

The last important global operation is execute-fold. In order to execute a fold along a line l1, all the agents in l1
make actuation requests to the simulator. The simulator approximates the configuration by computing the best-fit line
of the set of actuating agents. The simulator then determines whether the best-fit line is a justified approximation
for a given set of agents



at the OSL level there is no notion of
gradients, or even agents




An agent creates a gradient by sending a message to its
local neighborhood with the gradient name and a value of
zero. The neighboring agents forward the message to their
neighbors with the value incremented by one and so on, until the gradient has propagated over the entire sheet. Each
agent stores the minimum value it has heard for a particular
gradient name, thus the gradient value increases away from
the source. Because agents communicate with only neighboring agents within a small radius, the gradient provides
an estimate of distance from the source. Gradients are quite
general and similar primitives have been used in other contexts [5, 15, 3]2. The source of a gradient could be a group of
agents, in which case the gradient value reflects the shortest
distance to any of the sources. Thus, the shape and positions of the sources affects the spatial pattern of gradient
values. For example if a single cell emits a gradient then
the value increases as one moves radially away from the cell
but if a line of cells emit a gradient then the gradient value
increases as one moves perpendicularly away from the line



Cell-to-cell contact is the main way in which changes to
the environment (sheet) affect the behavior of the agents. It
allows multiple layers of the sheet to act as a single fused
layer. For example, cell-to-cell contact indirectly affects the
way gradients propagate by changing the local neighborhoods. Gradients seep through regions of the sheet that are
in contact with each other as if the sheet were a single layer


Flexible Folding: This primitive comes from the actuation model of the agent. An agent can contract its apical or
basal fibers in order to affect the shape of the sheet. In the
simulation environment, the requests from the agents to contract apical/basal surfaces are collected and then the simulator globally computes the result of those actuations. The
simulator also recalculates the cell-to-cell contact neighborhoods as a result of the change in sheet configuration. We
make this computation feasible by restricting the allowed
folds to straight flat folds